# Changelog

follows `yyyy-mm-dd` format and can have
headers of

`Added`, `Migration`/`Breaking Changes`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`,  `Internal`

## ongoing - 2026-05-03
TODOs document the added
### Added
- compile time checks for virtual tables are not supported. Added this specific error for better clarity.
- added `execute_many_runtime()` where we can run multiple chained sql statements (via `;`) at runtime.
- Added more robust error handling and suggestions for STRICT Table to get maximum benefits of this library. It will auto detect types that are valid but invalid in STRICT table and will suggest the correct type. It will also suggest using `CHECK (col in (0 or 1))` if you want to get `bool` type safety
- Generates an `init()` method if you are connecting via an external sql file. This allows to easily run whatever is defined in that sql file.
- added `_many` methods for write statements to easily have bulk operation without resorting to transactions
- added `transaction_immediate` for both runtime and compile time. Prefer that to  `transaction` if you are going to mix write and read stmts. If you are going to have purely read stmts, prefer `transaction`.

  - Internally,they are the same except `transaction immedate` starts transaction with `BEGIN IMMEDIATE` while `transasction` starts with `BEGIN DEFERRED`

### Fixed
- `CREATE TABLE` detection is now more robust by using AST parsing instead of string matching.


### Internal
- unify type mapping and remove redundant code
- Removed `check_constraint` field in `ColumnInfo` struct as it is no longer being used

## [0.2.2] - 2026-05-03

### Changed

The default PRAGMA settings when using the library are

```sql
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```
This gives best performance and high relibaility

## [0.2.1] - 2026-05-02
Documentation formatting

## [0.2.0] - 2026-05-02

### Changed

- Type casting between different types are now more strict. This is to prevent unexpected and unintuitive behavior at runtime.

Only the following casts are allowed:

    - Integer -> Real
    - Real -> Integer (Truncates towards zero)
    - Integer -> Text
    - Real -> Text
    - Bool -> Integer (true -> 1, false -> 0)
    - Bool -> Real (true -> 1.0, false -> 0.0)

### Migration

In previous versions, type casting was flexible. Any type could be casted to any other type. Upgrading to 0.2.0 introduces stricter rules that may break existing code. However, the changes are straightforward as the compiler will flag every affected line, making the fixes straightforward to apply.

## [0.1.1 - 0.1.10] - 2026-05-02
Docuemntation updates and improvements. No code changes.

## [0.1.0] - 2026-05-01

### Added

1. casting as BOOL is now supported
2. able to create table with BOOL datatype
3. BLOB are now fully supported.
4. pg `::` syntax remains and doesnt get translated to CAST AS when hovering over the function name in VSCode.
5. better error messages

### Changed

Mostly variable and macro names have been changed for better clarity, but features are identical

1. Library name changed from `lazysql` →`sqlitex`
2. `LazyConnection `→ `Connection`
3. `sql_runtime!()` → `sql_escape_hatch!()`
4. `execute_dynamic()` → `execute_runtime()
5. `query_dynamic()` → `query_runtime()`

## Earlier

Library was originally named LazySql. Other than the naming changes and additional features mentioned in `sqlitex` 0.1.0 release, features are the same.
