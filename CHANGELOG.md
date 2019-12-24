
# CHANGELOG

## v0.4.0 2019-12-24

#### Fix
* Tix tests

## v0.4.0-alpha.4 2019-12-24

#### Add
    * `Pool.set_max_open_conns` - Sets the maximum number of connections managed by the pool.
    * `Pool.set_max_idle_conns` - Sets the maximum idle connection count maintained by the pool.
    * `Pool.set_conn_max_lifetime` - Sets the maximum lifetime of connections in the pool.


## v0.4.0-alpha.3 2019-12-24

#### Update
* When specifying a timeout configuration, `None` means that a get connection will never timeout.

## v0.4.0-alpha.2 2019-12-23

#### Fixes
* Fix docs

## v0.4.0-alpha.1 2019-12-23

Refactored most of the code, but it was all tested. I'm sorry, some API changes, but in exchange for better experience and performance.

#### Changes
* ConnectionManager is changed to Manager and simplified.
  * `get_executor` are removed.
  * `has_broken` are removed.
  * `is_valid` to `check`
  * Add a provided method `spawn_task`. You might use it when using mobc in runtimes other than tokio-0.2 and async-std.
* The connection pool now initializes without creating a new database connection, which saves a lot of time and resources. So it becomes a synchronization API.
* `Pool.state()` will now return more detailed statistics.
* Some configuration items are changed.
  * `min_idle` to `max_idle`, which means that idle connections over this number will be closed
  * `max_size` to `max_open`. If you set it to 0, the pool will create unlimited new connections if necessary. However, these connections are still limited by `max_idle` when idle
* `Pool.try_get` are removed.


## v0.3.3 2019-12-23

#### Fixes

* Fix Connection recycle.
* Panics if `min_idle` is 0

## v0.3.2 2019-12-10

#### Fixes

* Timeout caused by reaping

## v0.2.11 2019-12-10

#### Fixes

* Timeout caused by reaping

## v0.3.1 2019-12-05

#### Fixes
    
* Documentation of `mobc::Builder`