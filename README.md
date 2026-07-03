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

