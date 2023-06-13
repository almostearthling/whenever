# The Whenever Task Scheduler

**Whenever** is a simple task scheduler capable of executing _tasks_ (OS commands and _Lua_ scripts) according to specific _conditions_. Conditions are of various types: depending on time (both intervals or specific points in time), execution of OS commands or _Lua_ scripts, changes in specific files and directories, session inactivity, DBus signals or property checks[^1]. The scheduler intends to be as lightweight as possible in terms of used computational resources, and to run at a low priority level.

Configuration is provided to the scheduler via a TOML file, which must contain all definitions for conditions and associated tasks, as well as events that the scheduler should listen to.

Ideally, **Whenever** is the successor of the _Python_ based [**When**](https://github.com/almostearthling/when-command) scheduler, with the intention of being cross platform, more efficient and as least resource-consuming as possible. It also gained some features (eg. _Lua_ scripting) that **When** did not have, at no cost in terms of performance since **Whenever** is a self-contained, optimized, and thin executable instead of being an interpreted program.

Although a command line application, it is designed for desktops - therefore it should be executed via a controlling GUI wrapper.


[^1]: Although DBus support is present on Windows too, it is mostly useful on Linux desktops.
