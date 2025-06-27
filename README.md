# The Whenever Task Scheduler

![HeaderImage](docs/graphics/metronome.png)


**whenever** is a simple task scheduler capable of executing _tasks_ (in particular, OS commands and _Lua_ scripts) according to specific _conditions_. Conditions are of various types: depending on time (both intervals or specific more-or-less defined instants), execution of OS commands or _Lua_ scripts, changes in specific files and directories, session inactivity, DBus signals or property checks, WMI queries on Windows. The scheduler intends to be as lightweight as possible in terms of used computational resources, and to possibly run at a low priority level.

Configuration is provided to the scheduler via a [TOML](https://toml.io/) file, which must contain all definitions for conditions and associated tasks, as well as events that the scheduler should listen to.

Although a command line application, it is designed for desktops -- therefore it should be executed via a controlling GUI wrapper. There are two frontends currently available:

* [When](https://github.com/almostearthling/when-command), a Python based fully featured application to configure and run **whenever**
* [whenever_tray](https://github.com/almostearthling/whenever_tray), a minimal wrapper that displays an icon in the system tray and provides basic interaction.

Please refer to the documentation for details.

