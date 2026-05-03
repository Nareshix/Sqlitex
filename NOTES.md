
# Some notes

fts5 hard to do compile time. just suse runtime features. might add an eaxmple in future.
technically faster than rusqlite. AUto cache stmts and pragma settings aside,  sqlitex has a faster archtiecture than rusqlite.  but it honestly doesnt matter becasuse in real world, there isnt any noticable difference. should i say it in readme? main highlight is the ergonomics.

sql_escape_hatch! can unironically be used for custom data type, useful for datetime like jiff/chrono/time.

e.g.

```rust
insert_event: sql_escape_hatch!(
    "INSERT INTO events (created_at) VALUES (?)",
    jiff::Timestamp  // user specifies this manually
)
```

but still need to add the impl in ToSql and FromSql

To allow CREATE TABLE stmts anywhere within the macro, we can scan all the create tables first and add it to memory. I did that originally but there is an issue. When we make a mistake in CREATE TABLE, and that stmt is somewhere in between multiple sql!() macros, the error wouldnt highlight the CREATE TABLE stmt, but isntead highlight the lines before it saying things like table do not exist. THis is extremley misleading and confusing, so for now just leave things as is until u come back to it later. U did document this in the quick_start and documentation so its good enough

1. for hover over funciton, need to test whether it works on other editors. works great on vscode

2. rn blob loads everything to memory. maybe add streaming support for blob?

3. https://github.com/rust-lang/rust/issues/54140, suggestions can be better wit hthis api rather than saying it in error




sql file and db file is read from Cargo.toml (the root of your project)

docuemtn sql_runtime_exec can be used for chained statements

transaction when hover ver should say wht it does with an example


test out how good the other 2 connection methods are.
generated method of init and transaction shoudl be more detailed, init should say panic if create table ecist. or smth
# Feature addition (subjected to confirmation)

1. bulk insert
db.transaction(|tx| {
    for item in &items {
        tx.add_user(item.id, &item.username, item.is_active)?;
    }
    Ok(())
})?;
fsync so its still bulk depsite transaction
maybe generate a _bulk method (simlar to python executemany) for write operations. but it can pollute the ide autosuggestions fast. think about runtime wise
2. begin immediate (https://sqlite.org/forum/forumpost/04ed1d235b)
3. chrono/time/jiff or other datetime-based library support
4. might allwo strict table to wrok well with bool, and deprecate the flexible type
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

# Others
sqlite optimize
VACCUm
https://www.sqlite.org/optoverview.html
https://sqlite.org/lang_vacuum.html
