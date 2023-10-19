# CHANGELOG

See [Releases](https://github.com/importcjj/mobc/releases) for the latest change log.

## v0.8.1

- Fix typo's in readme
- Update mobc-postgres and mobc-redis to use latest version

## v0.8.0

- Replaces channels with Sempahores
- Adds metrics

## v0.7.3 2021-6-25

- Support the actix-rt v1.0.

## v0.7.2 2021-4-2

- Retry when connection check fails on checkout

## v0.7.1 2021-3-12

- Optimize the internal get requests queue.

## v0.7.0 2021-1-19

- Compatible with Tokio 1.0

## v0.5.12 2020-07-08

- Add a new method `into_inner` to `connection` to unwrap the raw connection.

## v0.5.11 2020-6-17

- Upgrade the `future-timer`.

## v0.5.10 2020-6-3

- Add a sync hook `validate` in the `Manager` trait to _quickly_ determine connections are still valid when check-in

## v0.5.9 2020-6-3

- Fix get timeout.

## v0.5.8(yanked) 2020-6-1

- Add a hook to determine connections are valid when check-in.

## v0.5.7 2020-03-28

- Fix PANIC: overflow when subtracting duration from instant.

## v0.5.6 2020-03-26

- Fixes to health checks, add a check interval

## v0.5.5 2020-03-24

- Add a new option to set connection's max idle lifetime.

## v0.5.4 2020-03-17

- Do not run the `check` if `health_check` is false.

## v0.5.3 2020-01-17

- Fix performance regression.

## v0.5.2 2020-01-17

- Do health check for the connection before return it.
- Add configure item `health_check`.
- Impl Debug for State.
- Add method is_brand_new to Connection, which returns true if the connection is newly established.
- Skip health check of those new connections.

## v0.5.1 2020-01-07

- Switch to `async-trait`
- Switch to `futures-timer`
- Add more examples

## v0.4.1 2019-12-29

#### Add

- Export `spawn`;

## v0.4.0 2019-12-24

#### Fix

- Tix tests

## v0.4.0-alpha.4 2019-12-24

#### Add

    * `Pool.set_max_open_conns` - Sets the maximum number of connections managed by the pool.
    * `Pool.set_max_idle_conns` - Sets the maximum idle connection count maintained by the pool.
    * `Pool.set_conn_max_lifetime` - Sets the maximum lifetime of connections in the pool.

## v0.4.0-alpha.3 2019-12-24

#### Update

- When specifying a timeout configuration, `None` means that a get connection will never timeout.

## v0.4.0-alpha.2 2019-12-23

#### Fixes

- Fix docs

## v0.4.0-alpha.1 2019-12-23

Refactored most of the code, but it was all tested. I'm sorry, some API changes, but in exchange for better experience and performance.

#### Changes

- ConnectionManager is changed to Manager and simplified.
  - `get_executor` are removed.
  - `has_broken` are removed.
  - `is_valid` to `check`
  - Add a provided method `spawn_task`. You might use it when using mobc in runtimes other than tokio-0.2 and async-std.
- The connection pool now initializes without creating a new database connection, which saves a lot of time and resources. So it becomes a synchronization API.
- `Pool.state()` will now return more detailed statistics.
- Some configuration items are changed.
  - `min_idle` to `max_idle`, which means that idle connections over this number will be closed
  - `max_size` to `max_open`. If you set it to 0, the pool will create unlimited new connections if necessary. However, these connections are still limited by `max_idle` when idle
- `Pool.try_get` are removed.

## v0.3.3 2019-12-23

#### Fixes

- Fix Connection recycle.
- Panics if `min_idle` is 0

## v0.3.2 2019-12-10

#### Fixes

- Timeout caused by reaping

## v0.2.11 2019-12-10

#### Fixes

- Timeout caused by reaping

## v0.3.1 2019-12-05

#### Fixes

- Documentation of `mobc::Builder`
