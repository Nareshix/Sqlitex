## TODOS

# Some notes

technically faster than rusqlite. AUto cache stmts and pragma settings aside,  sqlitex has a faster archtiecture than rusqlite.  but it honestly doesnt matter becasuse in real world, there isnt any noticable difference. should i say it in readme? main highlight is the ergonomics.


To allow CREATE TABLE stmts anywhere within the macro, we can scan all the create tables first and add it to memory. I did that originally but there is an issue. When we make a mistake in CREATE TABLE, and that stmt is somewhere in between multiple sql!() macros, the error wouldnt highlight the CREATE TABLE stmt, but isntead highlight the lines before it saying things like table do not exist. THis is extremley misleading and confusing, so for now just leave things as is until u come back to it later. U did document this in the quick_start and documentation so its good enough


# Feature addition (subjected to confirmation)
1. for hover over funciton, need to test whether it works on other editors. works great on vscode


2. nutype support? but complex . realistically might not ever implement it but just leave it here as a food for thought.

3. rn blob loads everything to memory. maybe add streaming support for blob?

4. check_constraint field in SELECT is ignored for now.

5. upsert - INSERT OR REPLACE INTO users (id, name) VALUES (?, ?)

1. bulk insert
2. begin immediate
3. chrono/time/jiff or other datetime-based library support
4. better egonomic for bulk operation? maybe.
5. url crate?
6. it follows an opinionated API design
7. Doesn't support Batch Execution ergonomically. You would need to resort to `sql!()` or `sql_escape_hatch!()` macro


# internal code improvement concerns
sqlitex_type_inference crate needs some refactoring as the codebase is q messy
add contributing.md cuz u seem to forget wht u write lol then spenda lot of time looking though code to rmb wht u did. gonna make same mistake again rn :p


# automation conerns
try to keep packages up to date. theresa  github bot that notifies u for breking semver. try to often update for minor patches. does cargo update do this? automate it

add changelog and update git tags even if its jsut documentation change.

cargod deny check. cargo outdated, cargo update/upgrade .github depandabot alternative or renovate?
`
have mdbook and host the docs rather than relying on docs.rs page.
orrr, u can rewrite ur python ssg in rust and make it more smoother. :D
