## TODOS
1. rn blob loads everything to memory. maybe add streaming support for blob?

2. check_constarint field in SELECT is ignored for now. maybe in future will make use of this field via nutype/nnn

upsert - INSERT OR REPLACE INTO users (id, name) VALUES (?, ?)

1. bulk insert
2. begin immediate
3. chrono/time/jiff or other datetime-based library support
4. better egonomic for bulk operation? maybe.
5. url crate?
  1. it follows an opinionated API design
  2. Doesn't support Batch Execution ergonomically. You would need to resort to `sql!()` or `sql_escape_hatch!()` macro

//TODO sqlite3_busy_timeout does return an int. It is nearly a gurantee for this
// function to never fail. but its still good to handle it. If it fails mean
// the sql query is taking more than 5 second which means its inefficent lol
hence give eoption to change the timeout
make the readme shorter

in case CREATE TABLE is done after a random query in sql_struct should i allow it? like scan whole struct first instead of top down? at least show a warning

type_inference crate needs some refactoring as the codebase is q messy