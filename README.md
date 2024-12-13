# bevy-reflect-check

A tool for checking if all components in Bevy that are reflected are properly defined as ReflectionComponents.

## Usage

If you just call `cargo run`, it'll download Bevy 0.15.0 and check for this discrepancy, outputting all components that fail this test.

You can check a local version (or a different version) of Bevy by replacing the corresponding line in Cargo.toml.

## Why

Because [Bevy ticket #16659](https://github.com/bevyengine/bevy/issues/16659). Apparently this was not done properly a few times and there are no safeguards against the mistake.

## Copyright

Significant parts of the program were written by ChatGPT, so there is an assumption that this program does not constitute enough originality by an author to fall under copyright law.
