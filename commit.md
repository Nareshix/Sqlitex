feat: added native migration support

- auto-discovers and sort all .sql files numerically
- validates schema and syntax at compile-time for all of them
- creates _sqlitex_migration table
- checksum tamper protection for every file
- atomic (runs under transaction)
- immutable (can't rollabck to prev migration file, need to create a new one)
