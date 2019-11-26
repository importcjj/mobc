use mobc::futures;
use mobc::AnyFuture;
use mobc::ConnectionManager;
use mobc::Error;
use mobc::Pool;
use std::error;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mobc::runtime::{delay_for, Runtime, TaskExecutor};

#[derive(Debug, PartialEq)]
pub struct TestError;

impl fmt::Display for TestError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str("blammo")
    }
}

impl error::Error for TestError {
    fn description(&self) -> &str {
        "Error"
    }
}

#[derive(Debug, PartialEq)]
struct FakeConnection(bool);

struct OkManager {
    executor: TaskExecutor,
}

impl ConnectionManager for OkManager {
    type Connection = FakeConnection;
    type Error = TestError;
    type Executor = TaskExecutor;

    fn get_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(FakeConnection(true)))
    }

    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(conn))
    }

    fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
        false
    }
}

struct NthConnectFailManager {
    num: AtomicIsize,
    executor: TaskExecutor,
}

impl ConnectionManager for NthConnectFailManager {
    type Connection = FakeConnection;
    type Error = TestError;
    type Executor = TaskExecutor;

    fn get_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
        if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
            Box::pin(futures::future::ok(FakeConnection(true)))
        } else {
            Box::pin(futures::future::err(TestError))
        }
    }

    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(conn))
    }

    fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
        false
    }
}

#[test]
fn test_max_size_ok() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let handler = NthConnectFailManager {
            num: AtomicIsize::new(5),
            executor: rt.executor(),
        };
        let pool = Pool::builder().max_size(5).build(handler).await?;

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
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let handler = OkManager {
            executor: rt.executor(),
        };
        let pool = Pool::builder().max_size(2).build(handler).await?;

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

#[test]
fn test_try_get() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let handler = OkManager {
            executor: rt.executor(),
        };
        let pool = Pool::builder().max_size(2).build(handler).await?;

        let conn1 = pool.try_get().await;
        let conn2 = pool.try_get().await;
        let conn3 = pool.try_get().await;

        assert!(conn1.is_some());
        assert!(conn2.is_some());
        assert!(conn3.is_none());

        drop(conn1);
        delay_for(Duration::from_secs(1)).await;
        assert!(pool.try_get().await.is_some());
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_get_timeout() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let handler = OkManager {
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .max_size(1)
            .connection_timeout(Duration::from_millis(500))
            .build(handler)
            .await?;

        let timeout = Duration::from_millis(100);
        let succeeds_immediately = pool.get_timeout(timeout).await;
        assert!(succeeds_immediately.is_ok());

        rt.spawn(async move {
            delay_for(Duration::from_millis(50)).await;
            drop(succeeds_immediately);
        });

        let succeeds_delayed = pool.get_timeout(timeout).await;
        assert!(succeeds_delayed.is_ok());

        rt.spawn(async move {
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
    let rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.store(true, Ordering::SeqCst);
        }
    }

    struct Hanlder {
        executor: TaskExecutor,
    };

    impl ConnectionManager for Hanlder {
        type Connection = Connection;
        type Error = TestError;
        type Executor = TaskExecutor;

        fn get_executor(&self) -> Self::Executor {
            self.executor.clone()
        }

        fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(Connection))
        }

        fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }

        fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
            true
        }
    }

    rt.block_on(async {
        let handler = Hanlder {
            executor: rt.executor(),
        };
        let pool = Pool::new(handler).await?;

        drop(pool.get().await.ok().unwrap());
        delay_for(Duration::from_secs(1)).await;
        assert!(DROPPED.load(Ordering::SeqCst));
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_initialization_failure() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let handler = NthConnectFailManager {
            num: AtomicIsize::new(0),
            executor: rt.executor(),
        };
        let err = Pool::builder()
            .connection_timeout(Duration::from_secs(1))
            .build(handler)
            .await
            .err()
            .unwrap();

        match err {
            Error::Inner(TestError) => (),
            _ => panic!("error unexpected"),
        }

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_lazy_initialization_failure() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let handler = NthConnectFailManager {
            num: AtomicIsize::new(0),
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .connection_timeout(Duration::from_secs(1))
            .build_unchecked(handler)
            .await;

        let err = pool.get().await.err().unwrap();
        match err {
            Error::Timeout => (),
            _ => panic!("{:?} expected"),
        }

        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_get_global_timeout() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let handler = OkManager {
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .max_size(1)
            .connection_timeout(Duration::from_secs(1))
            .build(handler)
            .await?;

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
    let rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Hanlder {
        num: AtomicIsize,
        executor: TaskExecutor,
    };

    impl ConnectionManager for Hanlder {
        type Connection = Connection;
        type Error = TestError;
        type Executor = TaskExecutor;

        fn get_executor(&self) -> Self::Executor {
            self.executor.clone()
        }

        fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Box::pin(futures::future::ok(Connection))
            } else {
                Box::pin(futures::future::err(TestError))
            }
        }

        fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }

        fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
            false
        }
    }

    rt.block_on(async {
        let handler = Hanlder {
            num: AtomicIsize::new(5),
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .max_size(5)
            .idle_timeout(Some(Duration::from_secs(1)))
            .reaper_rate(Duration::from_secs(1))
            .build(handler)
            .await?;

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
    let rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Hanlder {
        num: AtomicIsize,
        executor: TaskExecutor,
    };

    impl ConnectionManager for Hanlder {
        type Connection = Connection;
        type Error = TestError;
        type Executor = TaskExecutor;

        fn get_executor(&self) -> Self::Executor {
            self.executor.clone()
        }

        fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Box::pin(futures::future::ok(Connection))
            } else {
                Box::pin(futures::future::err(TestError))
            }
        }

        fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }

        fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
            false
        }
    }

    rt.block_on(async {
        let handler = Hanlder {
            num: AtomicIsize::new(5),
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .max_size(5)
            .idle_timeout(Some(Duration::from_secs(1)))
            .reaper_rate(Duration::from_secs(1))
            .build(handler)
            .await?;

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
    let rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Hanlder {
        num: AtomicIsize,
        executor: TaskExecutor,
    };

    impl ConnectionManager for Hanlder {
        type Connection = Connection;
        type Error = TestError;
        type Executor = TaskExecutor;

        fn get_executor(&self) -> Self::Executor {
            self.executor.clone()
        }

        fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
            if self.num.fetch_sub(1, Ordering::SeqCst) > 0 {
                Box::pin(futures::future::ok(Connection))
            } else {
                Box::pin(futures::future::err(TestError))
            }
        }

        fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }

        fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
            false
        }
    }

    rt.block_on(async {
        let handler = Hanlder {
            num: AtomicIsize::new(5),
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .max_size(5)
            .max_lifetime(Some(Duration::from_secs(1)))
            .connection_timeout(Duration::from_secs(1))
            .reaper_rate(Duration::from_secs(1))
            .build(handler)
            .await?;

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
    let rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    struct Hanlder {
        executor: TaskExecutor,
    };

    impl ConnectionManager for Hanlder {
        type Connection = Connection;
        type Error = TestError;
        type Executor = TaskExecutor;

        fn get_executor(&self) -> Self::Executor {
            self.executor.clone()
        }

        fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(Connection))
        }

        fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }

        fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
            false
        }
    }

    rt.block_on(async {
        let handler = Hanlder {
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .max_size(5)
            .min_idle(Some(2))
            .build(handler)
            .await?;
        delay_for(Duration::from_secs(2)).await;

        assert_eq!(2_u32, pool.state().await.idle_connections);
        assert_eq!(2_u32, pool.state().await.connections);
        let pool = &pool;
        let mut conns = vec![];
        for _ in 0..3_u8 {
            conns.push(pool.get().await.unwrap());
        }

        assert_eq!(3, conns.len());
        delay_for(Duration::from_secs(1)).await;
        assert_eq!(2_u32, pool.state().await.idle_connections);
        assert_eq!(5_u32, pool.state().await.connections);
        std::mem::drop(conns);
        delay_for(Duration::from_secs(1)).await;
        assert_eq!(5_u32, pool.state().await.idle_connections);
        assert_eq!(5_u32, pool.state().await.connections);
        Ok::<(), Error<TestError>>(())
    })
    .unwrap();
}

#[test]
fn test_conns_drop_on_pool_drop() {
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    let rt: Runtime = Runtime::new().unwrap();

    struct Connection;

    impl Drop for Connection {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Hanlder {
        executor: TaskExecutor,
    };

    impl ConnectionManager for Hanlder {
        type Connection = Connection;
        type Error = TestError;
        type Executor = TaskExecutor;

        fn get_executor(&self) -> Self::Executor {
            self.executor.clone()
        }

        fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(Connection))
        }

        fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
            Box::pin(futures::future::ok(conn))
        }

        fn has_broken(&self, _conn: &mut Option<Self::Connection>) -> bool {
            false
        }
    }
    rt.block_on(async {
        let handler = Hanlder {
            executor: rt.executor(),
        };
        let pool = Pool::builder()
            .max_lifetime(Some(Duration::from_secs(10)))
            .max_size(10)
            .build(handler)
            .await?;

        drop(pool);
        for _ in 0..10_u8 {
            if DROPPED.load(Ordering::SeqCst) == 10 {
                return Ok::<(), Error<TestError>>(());
            }
            delay_for(Duration::from_secs(1)).await;
        }

        panic!("timed out waiting for connections to drop");
    })
    .unwrap();
}
