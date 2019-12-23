
# CHANGELOG

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

## v0.3.2 2019-12-10

#### Fixes

* Timeout caused by reaping

## v0.2.11 2019-12-10

#### Fixes

* Timeout caused by reaping

## v0.3.1 2019-12-05

#### Fixes
    
* Documentation of `mobc::Builder`