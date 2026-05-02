## TODOS

1. connection via sql file is does not play well with rust analyser. need to constantly restart server to show live changes. have not tested how it looks like in other editor. only tested in  vscode so far. might sound overkill and proabbly worng but can we try to make use of diff threads. or if tehresa  ncier way to refresh rust analsyer without full restart would be nice.
2. likewise for hover over funciton, need to test whether it works on other editors
4. casting is abit too flexible. it dont make sense for a string to be type casted as bool. and rn for some reason one would expect it ot betrue since theres smth but its giving us fasle. so wrong ans at that too. easier to disable

5. rn blob loads everything to memory. maybe add streaming support for blob?

6. check_constarint field in SELECT is ignored for now. maybe in future will make use of this field via nutype/nnn

upsert - INSERT OR REPLACE INTO users (id, name) VALUES (?, ?)

1. bulk insert
2. begin immediate
3. chrono/time/jiff or other datetime-based library support
4. better egonomic for bulk operation? maybe.
5. url crate?
6. it follows an opinionated API design
7. Doesn't support Batch Execution ergonomically. You would need to resort to `sql!()` or `sql_escape_hatch!()` macro

//TODO sqlite3_busy_timeout does return an int. It is nearly a gurantee for this
// function to never fail. but its still good to handle it. If it fails mean
// the sql query is taking more than 5 second which means its inefficent lol
hence give eoption to change the timeout
make the readme shorter

in case CREATE TABLE is done after a random query in sql_struct should i allow it? like scan whole struct first instead of top down? at least show a warning

sqlitex_type_inference crate needs some refactoring as the codebase is q messy
add contributing.md cuz u seem to forget wht u write lol then spenda lot of time looking though code to rmb wht u did. gonna make same mistake again rn :p

try to keep packages up to date. theresa  github bot that notifies u for breking semver. try to often update for minor patches. does cargo update do this? automate it

add changelog and update git tags even if its jsut documentation change.

cargod deny check. cargo outdated, cargo update/upgrade .github depandabot alternative or renovate?
