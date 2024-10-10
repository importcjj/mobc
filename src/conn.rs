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

use crate::metrics_utils::{
    GaugeGuard, ACTIVE_CONNECTIONS, CLOSED_TOTAL, IDLE_CONNECTIONS, OPENED_TOTAL, OPEN_CONNECTIONS,
};

pub(crate) struct ActiveConn<C> {
    inner: C,
    state: ConnState,
    _permit: OwnedSemaphorePermit,
    _active_connections_gauge: GaugeGuard,
}

impl<C> ActiveConn<C> {
    pub(crate) fn new(inner: C, permit: OwnedSemaphorePermit, state: ConnState) -> ActiveConn<C> {
        Self {
            inner,
            state,
            _permit: permit,
            _active_connections_gauge: GaugeGuard::increment(ACTIVE_CONNECTIONS),
        }
    }

    pub(crate) fn into_idle(self) -> IdleConn<C> {
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

    pub(crate) fn into_raw(self) -> C {
        self.inner
    }

    pub(crate) fn as_raw_ref(&self) -> &C {
        &self.inner
    }

    pub(crate) fn as_raw_mut(&mut self) -> &mut C {
        &mut self.inner
    }
}

pub(crate) struct IdleConn<C> {
    inner: C,
    state: ConnState,
    _idle_connections_gauge: GaugeGuard,
}

impl<C> IdleConn<C> {
    pub(crate) fn is_brand_new(&self) -> bool {
        self.state.brand_new
    }

    pub(crate) fn into_active(self, permit: OwnedSemaphorePermit) -> ActiveConn<C> {
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

    pub(crate) fn split_raw(self) -> (C, ConnSplit<C>) {
        (
            self.inner,
            ConnSplit::new(self.state, self._idle_connections_gauge),
        )
    }
}

pub(crate) struct ConnState {
    pub(crate) created_at: Instant,
    pub(crate) last_used_at: Instant,
    pub(crate) last_checked_at: Instant,
    pub(crate) brand_new: bool,
    total_connections_open: Arc<AtomicU64>,
    total_connections_closed: Arc<AtomicU64>,
    _open_connections_gauge: GaugeGuard,
}

impl ConnState {
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

impl Drop for ConnState {
    fn drop(&mut self) {
        self.total_connections_open.fetch_sub(1, Ordering::Relaxed);
        self.total_connections_closed
            .fetch_add(1, Ordering::Relaxed);
        counter!(CLOSED_TOTAL).increment(1);
    }
}

pub(crate) struct ConnSplit<C> {
    state: ConnState,
    gauge: GaugeGuard,
    _phantom: PhantomData<C>,
}

impl<C> ConnSplit<C> {
    fn new(state: ConnState, gauge: GaugeGuard) -> Self {
        Self {
            state,
            gauge,
            _phantom: PhantomData,
        }
    }

    pub(crate) fn restore(self, raw: C) -> IdleConn<C> {
        IdleConn {
            inner: raw,
            state: self.state,
            _idle_connections_gauge: self.gauge,
        }
    }
}
