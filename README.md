# The Whenever Task Scheduler

![HeaderImage](docs/graphics/metronome.png)


**whenever** is a lightweight scheduler and automation tool capable of executing _tasks_ (in particular, OS commands and _Lua_ scripts) according to specific _conditions_. Conditions can be of several types: depending on time (both intervals or specific more-or-less defined instants), execution of OS commands or _Lua_ scripts, changes in specific files and directories, session inactivity, DBus signals or property checks, WMI queries and events on Windows. The scheduler intends to remain as frugal as possible in terms of used computational resources, and to possibly run at a low priority level, while still providing high flexibility and configurability.

The scheduler can be configured via a [TOML](https://toml.io/) file, which must contain all definitions for conditions and associated tasks, as well as events that the scheduler should listen to while running in the background.

Although a command line application it is designed for desktops, therefore it should be executed via a controlling GUI frontend. Currently, there are two companion wrappers available:

* [When](https://github.com/almostearthling/when-command), a Python based fully featured application to configure and run **whenever**,
* [whenever_tray](https://github.com/almostearthling/whenever_tray), a minimal wrapper that displays an icon in the system tray and provides basic interaction.

Prebuilt binaries can be downloaded from the [releases](https://github.com/almostearthling/whenever/releases) page, and basic [installation instructions](https://almostearthling.github.io/whenever/90.install.html) can be found in the online documentation. However, the easiest (and suggested) way to get **whenever** up and running, is to [install When](https://almostearthling.github.io/when-command/install.html), use it to download and configure **whenever**, and set it up to start when the user session begins.

Please refer to the [documentation](https://almostearthling.github.io/whenever/index.html) for configuration and installation details.


[![pages-build-deployment](https://github.com/almostearthling/whenever/actions/workflows/pages/pages-build-deployment/badge.svg)](https://github.com/almostearthling/whenever/actions/workflows/pages/pages-build-deployment)
