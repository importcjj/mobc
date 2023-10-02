use futures_util::{
    future::{join, join_all, ready, FutureExt},
    stream, StreamExt, TryStreamExt,
};
use mobc::async_trait;
use mobc::delay_for;
use mobc::runtime::Runtime;
use mobc::Connection;
use mobc::Error;
use mobc::Manager;
use mobc::Pool;
use mobc::State;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub struct TestError;

#[derive(Debug, PartialEq)]
struct FakeConnection(bool);

struct OkManager;

#[async_trait]
impl Manager for OkManager {
    type Connection = FakeConnection;
    type Error = TestError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        Ok(FakeConnection(true))
    }

    async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
        Ok(conn)
    }
}

struct NthConnectFailManager {
    num: AtomicIsize,
}

#[async_trait]
impl Manager for NthConnectFailManager {
    type Connection = FakeConnection;
    type Error = TestError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
            Ok(FakeConnection(true))
        } else {
            Err(TestError)
        }
    }

    async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
        Ok(conn)
    }
}

/// Gets `count` connections from the pool, one at a time, dropping each connection
/// before getting the next.
///
/// Intended to assist with testing that connections get (or don't get) reused.
async fn get_and_drop_conns<M>(pool: &Pool<M>, count: usize)
where
    M: Manager,
    M::Error: Debug,
{
    for _ in 0..count {
        pool.get().await.unwrap();

        // Give the task spawned by dropping that connection a moment to work.
        delay_for(Duration::from_millis(100)).await;
    }
}

/// Gets `count` connections from the pool
async fn get_conns<M>(pool: &Pool<M>, count: usize) -> Result<Vec<Connection<M>>, Error<M::Error>>
where
    M: Manager,
{
    stream::repeat_with(|| pool.get())
        .take(count)
        .buffer_unordered(count)
        .try_collect()
        .await
}

#[test]
fn test_max_open_ok() {
    let mut rt = Runtime::new().unwrap();
    let handler = NthConnectFailManager {
        num: AtomicIsize::new(5),
    };
    rt.block_on(async {
        let pool = Pool::builder().max_open(5).build(handler);

        let mut conns = vec![];
        for _ in 0..5 {
            conns.push(pool.get().await.ok().unwrap());
        }
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_acquire_release() {
    let mut rt = Runtime::new().unwrap();
    let handler = OkManager;
    rt.block_on(async {
        let pool = Pool::builder().max_open(2).build(handler);

        let conn1 = pool.get().await.ok().unwrap();
        let conn2 = pool.get().await.ok().unwrap();
        drop(conn1);
        let conn3 = pool.get().await.ok().unwrap();
        drop(conn2);
        drop(conn3);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

// #[test]
// fn test_try_get() {
//     let mut rt = Runtime::new().unwrap();
//     let handler = OkManager;
//     rt.block_on(async {
//         let pool = Pool::builder().max_open(2).build(handler);

//         let conn1 = pool.try_get().await;
//         let conn2 = pool.try_get().await;
//         let conn3 = pool.try_get().await;

//         assert!(conn1.is_some());
//         assert!(conn2.is_some());
//         assert!(conn3.is_none());

//         drop(conn1);
//         delay_for(Duration::from_secs(1)).await;
//         assert!(pool.try_get().await.is_some());
//         Ok::<(), Error<TestError>>(())
//     })
//     .unwrap();
// }

#[test]
fn test_get_timeout() {
    let mut rt = Runtime::new().unwrap();
    let handle = rt.handle().clone();
    let handler = OkManager;
    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(1)
            .get_timeout(Some(Duration::from_millis(500)))
            .build(handler);

        let timeout = Duration::from_millis(100);
        let succeeds_immediately = pool.get_timeout(timeout).await;
        assert!(succeeds_immediately.is_ok());

        handle.spawn(async move {
            delay_for(Duration::from_millis(50)).await;
            drop(succeeds_immediately);
        });

        let succeeds_delayed = pool.get_timeout(timeout).await;
        assert!(succeeds_delayed.is_ok());

        handle.spawn(async move {
            delay_for(Duration::from_millis(150)).await;
            drop(succeeds_delayed);
        });

        let start = Instant::now();
        let fails = pool.get_timeout(timeout).await;
        println!("cost {:?}", start.elapsed());
        assert!(fails.is_err());

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_is_send_sync() {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<Pool<OkManager>>();
}

#[test]
fn test_drop_on_checkout() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    DROPPED.store(false, Ordering::SeqCst);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.store(true, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, _conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Err(TestError)
        }
    }
    let handler = Handler;

    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(handler);

        assert!(pool.get().await.is_ok());
        delay_for(Duration::from_secs(1)).await;
        assert!(!DROPPED.load(Ordering::SeqCst));

        assert!(pool.get().await.is_ok());
        assert!(DROPPED.load(Ordering::SeqCst));

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_drop_on_checkin() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    DROPPED.store(false, Ordering::SeqCst);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.store(true, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Ok(conn)
        }

        fn validate(&self, _conn: &mut Self::Connection) -> bool {
            false
        }
    }
    let handler = Handler;

    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(handler);

        let conn = pool.get().await?;
        assert!(!DROPPED.load(Ordering::SeqCst));
        drop(conn);
        delay_for(Duration::from_secs(1)).await;
        assert!(DROPPED.load(Ordering::SeqCst));

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_health_check_interval() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    DROPPED.store(false, Ordering::SeqCst);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.store(true, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, _conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Err(TestError)
        }
    }
    let handler = Handler;

    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(1)
            .test_on_check_out(true)
            .health_check_interval(Some(Duration::from_secs(1)))
            .build(handler);

        assert!(pool.get().await.is_ok());
        delay_for(Duration::from_millis(500)).await;
        assert!(!DROPPED.load(Ordering::SeqCst));

        assert!(pool.get().await.is_ok());
        assert!(!DROPPED.load(Ordering::SeqCst));

        delay_for(Duration::from_millis(600)).await;
        assert!(pool.get().await.is_ok());
        assert!(DROPPED.load(Ordering::SeqCst));

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_invalid_conn() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    DROPPED.store(false, Ordering::SeqCst);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.store(true, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, _conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Err(TestError)
        }
    }

    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(Handler);

        assert!(pool.get().await.is_ok());
        delay_for(Duration::from_secs(1)).await;
        assert!(pool.get().await.is_ok());
        assert!(DROPPED.load(Ordering::SeqCst));
        assert_eq!(1_u64, pool.state().await.connections);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_invalid_conn_recovery() {
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Handler {
        num: AtomicIsize,
    }

    #[async_trait]
    impl Manager for Handler {
        type Connection = FakeConnection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(FakeConnection(true))
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Ok(conn)
            } else {
                Err(TestError)
            }
        }
    }

    let handler = Handler {
        num: AtomicIsize::new(0),
    };
    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(handler);

        assert!(pool.get().await.is_ok());

        assert!(pool.get().await.is_ok());

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_lazy_initialization_failure() {
    let mut rt = Runtime::new().unwrap();
    let handler = NthConnectFailManager {
        num: AtomicIsize::new(0),
    };
    rt.block_on(async {
        let pool = Pool::builder()
            .get_timeout(Some(Duration::from_secs(1)))
            .build(handler);

        let err = pool.get().await.err().unwrap();
        match err {
            Error::Inner(TestError) => (),
            _ => panic!("expected"),
        }

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_get_global_timeout() {
    let mut rt = Runtime::new().unwrap();
    let handler = OkManager {};
    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(1)
            .get_timeout(Some(Duration::from_secs(1)))
            .build(handler);

        let _c = pool.get().await.unwrap();
        let started_waiting = Instant::now();
        pool.get().await.err().unwrap();
        assert_eq!(started_waiting.elapsed().as_secs(), 1);

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_idle_timeout_partial_use() {
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Handler {
        num: AtomicIsize,
    }

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Ok(Connection)
            } else {
                Err(TestError)
            }
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Ok(conn)
        }
    }
    let handler = Handler {
        num: AtomicIsize::new(5),
    };
    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(5)
            .max_lifetime(Some(Duration::from_secs(1)))
            .clean_rate(Duration::from_secs(1))
            .build(handler);

        for _i in 0..8_u8 {
            delay_for(Duration::from_millis(250)).await;
            pool.get().await.unwrap();
        }
        delay_for(Duration::from_millis(250)).await;

        // TODO
        // assert_eq!(4, DROPPED.load(Ordering::SeqCst));
        // assert_eq!(1, pool.state().await.connections);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_max_lifetime_lazy() {
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Ok(conn)
        }
    }
    let handler = Handler;
    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(5)
            .max_lifetime(Some(Duration::from_secs(1)))
            .get_timeout(Some(Duration::from_secs(1)))
            .clean_rate(Duration::from_secs(1))
            .build(handler);

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        drop(v);
        delay_for(Duration::from_secs(2)).await;

        let mut v = vec![];
        for _ in 0..4 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(5, DROPPED.load(Ordering::SeqCst));
        pool.get().await.unwrap();
        delay_for(Duration::from_secs(2)).await;
        assert_eq!(5, DROPPED.load(Ordering::SeqCst));
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_set_conn_max_lifetime() {
    env_logger::init();
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Handler {
        num: AtomicIsize,
    }

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Ok(Connection)
            } else {
                Err(TestError)
            }
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Ok(conn)
        }
    }
    let handler = Handler {
        num: AtomicIsize::new(5),
    };
    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(5)
            .max_idle(5)
            .max_lifetime(Some(Duration::from_secs(1)))
            .get_timeout(Some(Duration::from_secs(1)))
            .clean_rate(Duration::from_secs(1))
            .build(handler);

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }

        drop(v);

        let conn = pool.get().await.unwrap();
        assert_eq!(0, DROPPED.load(Ordering::SeqCst));
        delay_for(Duration::from_secs(2)).await;

        pool.set_conn_max_lifetime(Some(Duration::from_secs(1)))
            .await;
        delay_for(Duration::from_secs(4)).await;
        assert_eq!(4, DROPPED.load(Ordering::SeqCst));
        drop(conn);
        delay_for(Duration::from_secs(2)).await;
        assert_eq!(5, DROPPED.load(Ordering::SeqCst));

        assert_eq!(5_u64, pool.state().await.max_lifetime_closed);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_max_idle_lifetime() {
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Ok(conn)
        }
    }
    let handler = Handler;
    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(5)
            .max_idle(5)
            .max_idle_lifetime(Some(Duration::from_secs(1)))
            .get_timeout(Some(Duration::from_secs(1)))
            .test_on_check_out(false)
            .build(handler);

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(0, DROPPED.load(Ordering::SeqCst));
        drop(v);
        delay_for(Duration::from_millis(500)).await;

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(0, DROPPED.load(Ordering::SeqCst));
        drop(v);
        delay_for(Duration::from_millis(800)).await;

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(0, DROPPED.load(Ordering::SeqCst));
        drop(v);
        delay_for(Duration::from_millis(2000)).await;

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(5, DROPPED.load(Ordering::SeqCst));

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_set_max_open_conns() {
    let mut rt: Runtime = Runtime::new().unwrap();

    let handler = OkManager;

    rt.block_on(async {
        let pool = Pool::builder().max_open(5).max_idle(2).build(handler);

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(0_u64, pool.state().await.idle);
        assert_eq!(5_u64, pool.state().await.connections);
        assert_eq!(0_u64, pool.state().await.max_idle_closed);
        drop(v);

        delay_for(Duration::from_secs(1)).await;
        assert_eq!(2_u64, pool.state().await.idle);
        assert_eq!(2_u64, pool.state().await.connections);
        assert_eq!(3_u64, pool.state().await.max_idle_closed);
        let pool = &pool;
        let mut conns = vec![];
        for _ in 0..3_u8 {
            conns.push(pool.get().await.unwrap());
        }

        assert_eq!(3, conns.len());
        delay_for(Duration::from_secs(1)).await;
        assert_eq!(0_u64, pool.state().await.idle);
        assert_eq!(3_u64, pool.state().await.connections);
        assert_eq!(3_u64, pool.state().await.max_idle_closed);
        drop(conns);
        delay_for(Duration::from_secs(1)).await;
        assert_eq!(2_u64, pool.state().await.idle);
        assert_eq!(2_u64, pool.state().await.connections);
        assert_eq!(4_u64, pool.state().await.max_idle_closed);

        pool.set_max_open_conns(1).await;
        assert_eq!(1_u64, pool.state().await.max_open);
        assert_eq!(1_u64, pool.state().await.idle);
        assert_eq!(1_u64, pool.state().await.connections);
        assert_eq!(5_u64, pool.state().await.max_idle_closed);

        pool.set_max_open_conns(10).await;
        assert_eq!(10_u64, pool.state().await.max_open);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_set_max_idle_conns() {
    let mut rt: Runtime = Runtime::new().unwrap();

    let handler = OkManager;

    rt.block_on(async {
        let pool = Pool::builder().max_open(5).max_idle(5).build(handler);

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(0_u64, pool.state().await.idle);
        assert_eq!(5_u64, pool.state().await.connections);
        assert_eq!(0_u64, pool.state().await.max_idle_closed);
        drop(v);

        delay_for(Duration::from_secs(1)).await;
        pool.set_max_idle_conns(2).await;
        assert_eq!(5_u64, pool.state().await.max_open);
        assert_eq!(2_u64, pool.state().await.idle);
        assert_eq!(2_u64, pool.state().await.connections);
        assert_eq!(3_u64, pool.state().await.max_idle_closed);

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_min_idle() {
    let mut rt: Runtime = Runtime::new().unwrap();

    let handler = OkManager;

    rt.block_on(async {
        let pool = Pool::builder().max_open(5).max_idle(2).build(handler);

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }
        assert_eq!(0_u64, pool.state().await.idle);
        assert_eq!(5_u64, pool.state().await.connections);
        assert_eq!(0_u64, pool.state().await.max_idle_closed);
        drop(v);

        delay_for(Duration::from_secs(1)).await;
        assert_eq!(2_u64, pool.state().await.idle);
        assert_eq!(2_u64, pool.state().await.connections);
        assert_eq!(3_u64, pool.state().await.max_idle_closed);
        let pool = &pool;
        let mut conns = vec![];
        for _ in 0..3_u8 {
            conns.push(pool.get().await.unwrap());
        }

        assert_eq!(3, conns.len());
        delay_for(Duration::from_secs(1)).await;
        assert_eq!(0_u64, pool.state().await.idle);
        assert_eq!(3_u64, pool.state().await.connections);
        assert_eq!(3_u64, pool.state().await.max_idle_closed);
        drop(conns);
        delay_for(Duration::from_secs(1)).await;
        assert_eq!(2_u64, pool.state().await.idle);
        assert_eq!(2_u64, pool.state().await.connections);
        assert_eq!(4_u64, pool.state().await.max_idle_closed);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

/// Gets and drops the specified number of connections and checks that the
/// `Pool` has the expected state.
///
/// There is one field not checked in the expected `State`:
/// * `wait_duration`
async fn check_get_and_drop<M>(pool: &Pool<M>, take_conns: usize, expect: State)
where
    M: Manager,
    M::Error: Debug,
{
    println!(
        "Pool configured for max_open == {}; taking {} conns",
        pool.state().await.max_open,
        take_conns,
    );

    get_and_drop_conns(pool, take_conns).await;

    let state = pool.state().await;
    println!("Pool state: {state:#?}");
    assert_eq!(expect.max_open, state.max_open);
    assert_eq!(expect.connections, state.connections);
    assert_eq!(expect.in_use, state.in_use);
    assert_eq!(expect.idle, state.idle);
    assert_eq!(expect.wait_count, state.wait_count);
    assert_eq!(expect.max_idle_closed, state.max_idle_closed);
    assert_eq!(expect.max_lifetime_closed, state.max_lifetime_closed);
}

/// Ensure a pool configured for unlimited max_open and max_idle reuses connections.
#[test]
fn test_max_idle_and_max_open_unlimited() {
    let mut rt = Runtime::new().unwrap();
    rt.block_on(async {
        // Check that the builder produces the correct behavior
        let pool = Pool::builder().max_open(0).max_idle(0).build(OkManager);
        check(pool).await;
        println!("----");

        // Check that the `.set_max_*()` methods produce the correct behavior
        let pool = Pool::builder().max_open(10).max_idle(10).build(OkManager);
        pool.set_max_open_conns(0).await;
        pool.set_max_idle_conns(0).await;
        check(pool).await;
        println!("----");

        // Check that calling `.set_max_*()` in the reverse order produces the same result.
        let pool = Pool::builder().max_open(10).max_idle(10).build(OkManager);
        pool.set_max_idle_conns(0).await;
        pool.set_max_open_conns(0).await;
        check(pool).await;

        async fn check(pool: Pool<OkManager>) {
            check_get_and_drop(
                &pool,
                5,
                State {
                    max_open: 0,
                    connections: 1,
                    idle: 1,
                    in_use: 0,
                    max_idle_closed: 0,
                    max_lifetime_closed: 0,
                    wait_count: 0,
                    wait_duration: Duration::from_secs(0),
                },
            )
            .await;

            let conns = get_conns(&pool, 10).await;
            let state = pool.state().await;
            println!("Pool state with 10 conns in use: {state:#?}");
            assert_eq!(10, state.connections);
            assert_eq!(0, state.idle);
            assert_eq!(10, state.in_use);
            assert_eq!(0, state.max_idle_closed);
            assert_eq!(0, state.max_lifetime_closed);
            assert_eq!(0, state.wait_count);

            drop(conns);

            // Let the background tasks from the drop complete.
            delay_for(Duration::from_millis(100)).await;

            let state = pool.state().await;
            println!("Pool state after dropping all conns: {state:#?}");
            assert_eq!(10, state.connections);
            assert_eq!(10, state.idle);
            assert_eq!(0, state.in_use);
            assert_eq!(0, state.max_idle_closed);
            assert_eq!(0, state.max_lifetime_closed);
            assert_eq!(0, state.wait_count);
        }

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

/// Ensure a pool configured for unlimited max_open and limited max_idle reuses connections.
#[test]
fn test_max_idle_limited_max_open_unlimited() {
    let mut rt = Runtime::new().unwrap();
    rt.block_on(async {
        let pool = Pool::builder().max_open(0).max_idle(5).build(OkManager);
        check(pool).await;
        println!("----");

        // Check that the `.set_max_*()` methods produce the correct behavior
        let pool = Pool::builder().max_open(10).max_idle(10).build(OkManager);
        pool.set_max_open_conns(0).await;
        pool.set_max_idle_conns(5).await;
        check(pool).await;
        println!("----");

        // Check that calling `.set_max_*()` in the reverse order produces the same result.
        let pool = Pool::builder().max_open(10).max_idle(10).build(OkManager);
        pool.set_max_idle_conns(5).await;
        pool.set_max_open_conns(0).await;
        check(pool).await;

        async fn check(pool: Pool<OkManager>) {
            check_get_and_drop(
                &pool,
                6,
                State {
                    max_open: 0,
                    connections: 1,
                    idle: 1,
                    in_use: 0,
                    max_idle_closed: 0,
                    max_lifetime_closed: 0,
                    wait_count: 0,
                    wait_duration: Duration::from_secs(0),
                },
            )
            .await;

            // Make it so there's one more open conns than there are allowed idle conns
            let conns = get_conns(&pool, 6).await.unwrap();
            let state = pool.state().await;
            println!("Pool state with 6 conns in use: {state:#?}");
            assert_eq!(6, state.connections);
            assert_eq!(6, state.in_use);
            assert_eq!(0, state.idle);

            // Return them all to the pool.
            drop(conns);

            // A task is created for each dropped connection. Wait a moment to allow them to complete.
            delay_for(Duration::from_millis(100)).await;

            // Ensure the state of the pool is correct
            let state = pool.state().await;
            println!("Pool state after dropping all conns: {state:#?}");
            assert_eq!(5, state.connections);
            assert_eq!(0, state.in_use);
            assert_eq!(5, state.idle);
            assert_eq!(1, state.max_idle_closed);
        }

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

/// Check that a builder configured for more idle conns than open conns
/// uses the value for open conns as the max.
#[test]
fn test_max_idle_greater_than_max_open() {
    let mut rt = Runtime::new().unwrap();
    rt.block_on(async {
        let pool = Pool::builder().max_open(5).max_idle(6).build(OkManager);
        check(pool).await;
        println!("----");

        // Check that the `.set_max_*()` methods produce the correct behavior
        let pool = Pool::builder().max_open(10).max_idle(10).build(OkManager);
        pool.set_max_open_conns(5).await;
        pool.set_max_idle_conns(6).await;
        check(pool).await;
        println!("----");

        // Check that calling `.set_max_*()` in the reverse order produces the same result.
        let pool = Pool::builder().max_open(10).max_idle(10).build(OkManager);
        pool.set_max_idle_conns(6).await;
        pool.set_max_open_conns(5).await;
        check(pool).await;

        async fn check(pool: Pool<OkManager>) {
            let conns = get_conns(&pool, 5).await.unwrap();

            let state = pool.state().await;
            println!("With 5 connections in use: {state:#?}");

            assert_eq!(5, state.max_open);
            assert_eq!(5, state.connections);
            assert_eq!(5, state.in_use);
            assert_eq!(0, state.idle);

            drop(conns);
            delay_for(Duration::from_millis(100)).await;

            let state = pool.state().await;
            println!("After dropping all connections: {state:#?}");

            assert_eq!(5, state.connections);
            assert_eq!(0, state.in_use);
            assert_eq!(5, state.idle);
        }

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_conns_drop_on_pool_drop() {
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Ok(conn)
        }
    }
    let handler = Handler;
    rt.block_on(async {
        let pool = Pool::builder()
            .max_lifetime(Some(Duration::from_secs(10)))
            .max_idle(10)
            .max_open(10)
            .build(handler);

        let mut v = vec![];
        for _ in 0..10 {
            v.push(pool.get().await.unwrap());
        }
        drop(v);

        drop(pool);
        for _ in 0..10_u8 {
            log::debug!("{}", DROPPED.load(Ordering::SeqCst));
            if DROPPED.load(Ordering::SeqCst) == 10 {
                return Ok::<(), Error<TestError>>(());
            }
            delay_for(Duration::from_secs(1)).await;
        }

        panic!("timed out waiting for connections to drop");
    })
    .unwrap();
}

#[test]
fn test_is_brand_new() {
    let mut rt = Runtime::new().unwrap();
    let handler = OkManager;
    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(handler);

        let conn = pool.get().await.ok().unwrap();
        assert!(conn.is_brand_new());
        drop(conn);
        let conn = pool.get().await.ok().unwrap();
        assert!(!conn.is_brand_new());
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_unwraps_raw_conn() {
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            Ok(Connection)
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            Ok(conn)
        }
    }
    let handler = Handler;
    rt.block_on(async {
        let pool = Pool::builder()
            .max_lifetime(Some(Duration::from_secs(10)))
            .max_idle(10)
            .max_open(10)
            .build(handler);

        let mut v = vec![];
        for _ in 0..10 {
            let conn = pool.get().await.unwrap();
            let raw = conn.into_inner();
            v.push(raw);
        }
        pool.get().await.unwrap();
        delay_for(Duration::from_secs(1)).await;

        assert_eq!(1_u64, pool.state().await.idle);
        assert_eq!(1_u64, pool.state().await.connections);

        drop(v);
        assert_eq!(DROPPED.load(Ordering::SeqCst), 10);

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_timeout_when_db_has_gone() {
    struct Connection;
    struct Handler;

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            futures_util::future::pending::<()>().await;
            Ok(Connection)
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            futures_util::future::pending::<()>().await;
            Ok(conn)
        }
    }

    let mut rt = Runtime::new().unwrap();
    rt.block_on(async {
        const GET_TIMEOUT: Duration = Duration::from_secs(1);
        let pool = Pool::builder()
            .get_timeout(Some(GET_TIMEOUT))
            .build(Handler);

        let start = Instant::now();
        assert!(pool.get().await.is_err());
        assert!(start.elapsed() > GET_TIMEOUT);
        assert!(start.elapsed() < GET_TIMEOUT + Duration::from_millis(100));
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_timeout_when_db_has_gone2() {
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;
    struct Connection;
    struct Handler(AtomicU32);

    #[async_trait]
    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        async fn connect(&self) -> Result<Self::Connection, Self::Error> {
            if self.0.load(Ordering::Relaxed) > 0 {
                futures_util::future::pending::<()>().await;
            } else {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Connection)
        }

        async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
            if self.0.load(Ordering::Relaxed) > 0 {
                futures_util::future::pending::<()>().await;
            }
            Ok(conn)
        }
    }

    let mut rt = Runtime::new().unwrap();
    rt.block_on(async {
        const GET_TIMEOUT: Duration = Duration::from_secs(2);
        let handler = Handler(AtomicU32::new(0));
        let pool = Pool::builder()
            .get_timeout(Some(GET_TIMEOUT))
            .build(handler);

        assert!(pool.get().await.is_ok());

        let start = Instant::now();
        assert!(pool.get().await.is_err());
        assert!(start.elapsed() > GET_TIMEOUT);
        assert!(start.elapsed() < GET_TIMEOUT + Duration::from_millis(100));
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_requests_fifo() {
    let mut rt = Runtime::new().unwrap();
    let handler = OkManager;
    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(handler);

        let first = pool.get().await;
        assert!(first.is_ok());

        let second = pool.get().then(|conn| {
            assert!(conn.is_ok());
            drop(conn);
            ready(Instant::now())
        });

        let third = pool.get().then(|conn| {
            assert!(conn.is_ok());
            drop(conn);
            ready(Instant::now())
        });

        drop(first);
        let result = join(second, third).await;
        assert!(result.0 < result.1);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_requests_skip_canceled() {
    let mut rt = Runtime::new().unwrap();
    let handler = OkManager;
    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(handler);

        let first = pool.get().await;
        assert!(first.is_ok());

        let second = pool.get();
        drop(second);

        let third = pool.get();

        drop(first);
        assert!(third.await.is_ok());
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[cfg(all(
    feature = "tokio",
    not(any(feature = "async-std", feature = "actix-rt"))
))]
#[test]
fn test_handle_large_number_of_requests() {
    let mut rt = Runtime::new().unwrap();
    let handler = OkManager;
    rt.block_on(async {
        let pool = Pool::builder().max_open(10).build(handler);

        for _ in 1..5 {
            let mut futures = Vec::new();

            for _ in 1..50000 {
                let p1 = pool.clone();
                let fut = tokio::task::spawn(async move {
                    let conn = p1.get().await;
                    if conn.is_ok() {
                        assert!(conn.unwrap().0);
                    }
                    delay_for(Duration::from_millis(5)).await;
                });
                futures.push(fut);
            }
            delay_for(Duration::from_millis(50)).await;
            futures.iter().for_each(|fut| fut.abort());
            delay_for(Duration::from_millis(50)).await;
        }

        let mut futures = Vec::new();
        for _ in 1..10 {
            let p1 = pool.clone();
            let fut = tokio::task::spawn(async move {
                let conn = p1.get().await;
                assert!(conn.is_ok());
            });

            futures.push(fut);
        }

        let results = join_all(futures).await;
        for res in results {
            assert!(res.is_ok())
        }

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}
