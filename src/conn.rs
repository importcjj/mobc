use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use metrics::counter;
use tokio::sync::OwnedSemaphorePermit;

use crate::{
    metrics_utils::{
        GaugeGuard, ACTIVE_CONNECTIONS, CLOSED_TOTAL, IDLE_CONNECTIONS, OPENED_TOTAL,
        OPEN_CONNECTIONS,
    },
    Manager,
};

pub(crate) struct ActiveConn<M: Manager> {
    inner: M::Connection,
    state: ConnState<M>,
    _permit: OwnedSemaphorePermit,
    _active_connections_gauge: GaugeGuard<M>,
}

impl<M: Manager> ActiveConn<M> {
    pub(crate) fn new(
        inner: M::Connection,
        permit: OwnedSemaphorePermit,
        state: ConnState<M>,
    ) -> ActiveConn<M> {
        Self {
            inner,
            state,
            _permit: permit,
            _active_connections_gauge: GaugeGuard::increment(ACTIVE_CONNECTIONS),
        }
    }

    pub(crate) fn into_idle(self) -> IdleConn<M> {
        IdleConn {
            inner: self.inner,
            state: self.state,
            _idle_connections_gauge: GaugeGuard::increment(IDLE_CONNECTIONS),
        }
    }

    pub(crate) fn is_brand_new(&self) -> bool {
        self.state.brand_new
    }

    pub(crate) fn set_brand_new(&mut self, brand_new: bool) {
        self.state.brand_new = brand_new;
    }

    pub(crate) fn into_raw(self) -> M::Connection {
        self.inner
    }

    pub(crate) fn as_raw_ref(&self) -> &M::Connection {
        &self.inner
    }

    pub(crate) fn as_raw_mut(&mut self) -> &mut M::Connection {
        &mut self.inner
    }
}

pub(crate) struct IdleConn<M: Manager> {
    inner: M::Connection,
    state: ConnState<M>,
    _idle_connections_gauge: GaugeGuard<M>,
}

impl<M: Manager> IdleConn<M> {
    pub(crate) fn is_brand_new(&self) -> bool {
        self.state.brand_new
    }

    pub(crate) fn into_active(self, permit: OwnedSemaphorePermit) -> ActiveConn<M> {
        ActiveConn::new(self.inner, permit, self.state)
    }

    pub(crate) fn created_at(&self) -> Instant {
        self.state.created_at
    }

    pub(crate) fn expired(&self, timeout: Option<Duration>) -> bool {
        timeout
            .and_then(|check_interval| {
                Instant::now()
                    .checked_duration_since(self.state.created_at)
                    .map(|dur_since| dur_since >= check_interval)
            })
            .unwrap_or(false)
    }

    pub(crate) fn idle_expired(&self, timeout: Option<Duration>) -> bool {
        timeout
            .and_then(|check_interval| {
                Instant::now()
                    .checked_duration_since(self.state.last_used_at)
                    .map(|dur_since| dur_since >= check_interval)
            })
            .unwrap_or(false)
    }

    pub(crate) fn needs_health_check(&self, timeout: Option<Duration>) -> bool {
        timeout
            .and_then(|check_interval| {
                Instant::now()
                    .checked_duration_since(self.state.last_checked_at)
                    .map(|dur_since| dur_since >= check_interval)
            })
            .unwrap_or(true)
    }

    pub(crate) fn mark_checked(&mut self) {
        self.state.last_checked_at = Instant::now()
    }

    pub(crate) fn split_raw(self) -> (M::Connection, ConnSplit<M>) {
        (
            self.inner,
            ConnSplit::new(self.state, self._idle_connections_gauge),
        )
    }
}

pub(crate) struct ConnState<M: Manager> {
    pub(crate) created_at: Instant,
    pub(crate) last_used_at: Instant,
    pub(crate) last_checked_at: Instant,
    pub(crate) brand_new: bool,
    total_connections_open: Arc<AtomicU64>,
    total_connections_closed: Arc<AtomicU64>,
    _open_connections_gauge: GaugeGuard<M>,
}

impl<M: Manager> ConnState<M> {
    pub(crate) fn new(
        total_connections_open: Arc<AtomicU64>,
        total_connections_closed: Arc<AtomicU64>,
    ) -> Self {
        counter!(OPENED_TOTAL).increment(1);
        Self {
            created_at: Instant::now(),
            last_used_at: Instant::now(),
            last_checked_at: Instant::now(),
            brand_new: true,
            total_connections_open,
            total_connections_closed,
            _open_connections_gauge: GaugeGuard::increment(OPEN_CONNECTIONS),
        }
    }
}

impl<M: Manager> Drop for ConnState<M> {
    fn drop(&mut self) {
        self.total_connections_open.fetch_sub(1, Ordering::Relaxed);
        self.total_connections_closed
            .fetch_add(1, Ordering::Relaxed);
        counter!(CLOSED_TOTAL).increment(1);
    }
}

pub(crate) struct ConnSplit<M: Manager> {
    state: ConnState<M>,
    gauge: GaugeGuard<M>,
    _phantom: PhantomData<M>,
}

impl<M: Manager> ConnSplit<M> {
    fn new(state: ConnState<M>, gauge: GaugeGuard<M>) -> Self {
        Self {
            state,
            gauge,
            _phantom: PhantomData,
        }
    }

    pub(crate) fn restore(self, raw: M::Connection) -> IdleConn<M> {
        IdleConn {
            inner: raw,
            state: self.state,
            _idle_connections_gauge: self.gauge,
        }
    }
}
