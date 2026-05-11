document build.rs


```rust
fn main() {
    // This tells Cargo: "If anything in the migrations folder changes
    // (content edited, file added, or file deleted), re-compile the project."
    println!("cargo:rerun-if-changed=migrations");
}
```
