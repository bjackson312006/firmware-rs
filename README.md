# firmware-rs

This is the location of all drivers or code which is module and platform agnostic.  

This is perfect for anything implementing embedded hal, altough it could also contain embassy specific code (as a seperate crate).  

Things of note:  
- If a crate for the driver already exists but requires modification or replacement, recreate it here as <name>-ner  
- Ensure to use workspace dependencies if possible
- Use async functions when possible according to embedded-hal-async
- If an external dependency is used twice, consider making it a workspace dependency
- Add `#![no_std]` to `lib.rs`
- Use `defmt` workspace dep if logging or prints are needed

## Directory Structure

### Drivers
This is the location of all drivers or code which is module and platform agnostic.  

This is perfect for anything implementing embedded hal, altough it could also contain embassy specific code (as a seperate crate).  

Things of note:  
- If a crate for the driver already exists but requires modification or replacement, recreate it here as <name>-ner  
- Ensure to use workspace dependencies if possible
- Use async functions when possible according to embedded-hal-async
- If an external dependency is used twice, consider making it a workspace dependency
- Add `#![no_std]` to `lib.rs`
- Use `defmt` workspace dep if logging or prints are needed

## Directory Structure

### Drivers

Hardware-interacting drivers capable of implementing embedded-hal or at worst embassy.

### Platform

Drivers or utilities that must import stm32 specific functionality.  
In there try to provide different builds for different STMs.

### Utilities

Tools or design patterns that could be used across various projects.

### Middleware

Services (like shared logic threads), that could be used given the right imports.

Hardware-interacting drivers capable of implementing embedded-hal or at worst embassy.




# firmware-rs
NER Firmware in Rust (experimental, not for usage on car)


## Setup

1. [get rustup](https://www.rust-lang.org/learn/get-started).
2. clone and open this root folder.
3. [get probe-rs](https://probe.rs/docs/getting-started/installation/).

## Commands

- To enter a project: `cd ./projects/project-name`
- To build: `cargo build`
- To deploy onto an embedded chip locally connected, run `cargo run --release`.
- To format, run `cargo format`. 
- To lint and check stuff, run `cargo clippy`.
- To run a RTT terminal dedicated: `cargo embed --release rtt`
- To run a GDB terminal dedicated: `cargo embed --release gdb`
- To flash and leave code: `cargo embed --release`

**At this time, many commands only work consistently inside the project (via `cd`)**
The workspace's only purpose is to organize dependencies and build artificats, especially for rust-analyzer.

### Coding tips and tricks

- Use defmt macros to print stuff
- 

### IDE Stuff

There are currently custom rust-analyzer settings for VSCode and zed.  Feel free to adapt them to your own liking.


## Repository structure

This is a mono-repo configured as a normal embassy-styled project with multiple dependency crates and sub-projects.

Various files:

Top level `Cargo.toml` and `rust-toolchain.toml` define the various parts of the embassy project.  See comments inside for how these were structure, but most follow embassy specification.

The `crates` folder defines drivers or other code shared between projects.

The `projects` folder defines individual board-specific compilation units which inherit explicity defined `Cargo.toml` dependencies and `Embed.toml` settings, and more.  They can also depend on a crate in the `crates` folder.  Notably, they all may have individual `.config/cargo.toml` if they override anything.

This structure has multiple benefits, including:
- Static versioning of all embassy and other dependencies, eliminating version conflicts for in-tree code
- Inherited build settings so like-microcontroller projects share all of that boilerplate
- Shared `target` folder meaning a shared build cache for quicker and space-saving builds
- Other quirks, such as vscode `settings.json` and `config.toml`, are shared between projects


### Upgrades of Embassy/Dep versions

Updating versions is as follows

1. Update the rust-toolchain to the version found in embassy repo
2. Update all embassy versions to the versions found in the embassy repo, use x.y.z, in main Cargo.toml
3. Update all features, especially ones that say the version in them (ex. "defmt-03")
3. Update major package versions of other projects, use x.y, in main Cargo.toml
4. Update package versions of other projects, in individual Cargo.toml
4. Fix any build issues
