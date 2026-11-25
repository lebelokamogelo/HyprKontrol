# Contributing

Bug reports, themes and panels are welcome.

## Reporting a problem

Include the output of **About → Copy details** and the line shown in the bar at
the bottom of the window. It names the command that failed.

## Before you open a pull request

```sh
make check
```

This runs the formatter, clippy with warnings as errors, and the tests.

- Read [Architecture](docs/ARCHITECTURE.md) first. It is short.
- Anything that parses the output of a tool needs a test with real output.
- A change to the system needs an inverse, unless it truly has none.
- Arguments go into a `Cmd` as a list. A value that reaches a shell must be
  validated where the command is built.
- Secrets never go into arguments. Use standard input or D-Bus.
- Package operations are a `Session`. Do not pass `--noconfirm`.

## Style

- Comments explain why. Skip the ones that repeat the code, and take the space
  you need for the ones that record a decision or a pitfall.
- Interface text is plain and specific. Name what happens, say what went wrong
  and what to do about it.
- Colours, sizes and fonts come from the theme. A literal in a panel is a bug.

## Themes

A theme proposed for inclusion must pass `hyprkontrol --check-theme`, and needs
a licence that allows it to be shipped under MIT.
