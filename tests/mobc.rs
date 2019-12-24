use mobc::delay_for;
use mobc::runtime::Runtime;
use mobc::Error;
use mobc::Manager;
use mobc::Pool;
use mobc::ResultFuture;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub struct TestError;

#[derive(Debug, PartialEq)]
struct FakeConnection(bool);

struct OkManager;

impl Manager for OkManager {
    type Connection = FakeConnection;
    type Error = TestError;

    fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(FakeConnection(true)))
    }

    fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(conn))
    }
}

struct NthConnectFailManager {
    num: AtomicIsize,
}

impl Manager for NthConnectFailManager {
    type Connection = FakeConnection;
    type Error = TestError;

    fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
        if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
            Box::pin(futures::future::ok(FakeConnection(true)))
        } else {
            Box::pin(futures::future::err(TestError))
        }
    }

    fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(conn))
    }
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
fn test_drop_on_broken() {
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

    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(Connection))
        }

        fn check(&self, _conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::err(TestError))
        }
    }
    let handler = Handler;
    rt.block_on(async {
        let pool = Pool::new(handler);

        drop(pool.get().await.ok().unwrap());
        delay_for(Duration::from_secs(1)).await;
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

    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(Connection))
        }

        fn check(&self, _conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::err(TestError))
        }
    }

    rt.block_on(async {
        let pool = Pool::builder().max_open(1).build(Handler);

        drop(pool.get().await.ok().unwrap());
        delay_for(Duration::from_secs(1)).await;
        assert!(DROPPED.load(Ordering::SeqCst));
        assert_eq!(1_u64, pool.state().await.connections);
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
            _ => panic!("{:?} expected"),
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
fn test_idle_timeout() {
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
    };

    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Box::pin(futures::future::ok(Connection))
            } else {
                Box::pin(futures::future::err(TestError))
            }
        }

        fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }
    }
    let handler = Handler {
        num: AtomicIsize::new(5),
    };
    rt.block_on(async {
        let pool = Pool::builder()
            .max_open(5)
            .max_idle(Some(2))
            .max_lifetime(Some(Duration::from_secs(1)))
            .clean_rate(Duration::from_secs(1))
            .build(handler);

        let mut v = vec![];
        for _ in 0..5 {
            v.push(pool.get().await.unwrap());
        }

        drop(v);

        let conn = pool.get().await.unwrap();
        delay_for(Duration::from_secs(3)).await;
        assert_eq!(4, DROPPED.load(Ordering::SeqCst));
        drop(conn);
        assert_eq!(4, DROPPED.load(Ordering::SeqCst));
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
    };

    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Box::pin(futures::future::ok(Connection))
            } else {
                Box::pin(futures::future::err(TestError))
            }
        }

        fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
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
fn test_max_lifetime() {
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
    };

    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Box::pin(futures::future::ok(Connection))
            } else {
                Box::pin(futures::future::err(TestError))
            }
        }

        fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }
    }
    let handler = Handler {
        num: AtomicIsize::new(5),
    };
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

        let conn = pool.get().await.unwrap();
        delay_for(Duration::from_secs(2)).await;
        assert_eq!(4, DROPPED.load(Ordering::SeqCst));
        drop(conn);
        delay_for(Duration::from_secs(2)).await;
        assert_eq!(5, DROPPED.load(Ordering::SeqCst));
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_min_idle() {
    let mut rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    struct Handler;

    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(Connection))
        }

        fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }
    }

    let handler = Handler;

    rt.block_on(async {
        let pool = Pool::builder().max_open(5).max_idle(Some(2)).build(handler);

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

    impl Manager for Handler {
        type Connection = Connection;
        type Error = TestError;

        fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(Connection))
        }

        fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }
    }
    let handler = Handler;
    rt.block_on(async {
        let pool = Pool::builder()
            .max_lifetime(Some(Duration::from_secs(10)))
            .max_idle(Some(10))
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
