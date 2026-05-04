# bell

Lightweight notification daemon for Wayland systems.

## Table of contents

- [bell](#bell)
  - [Table of contents](#table-of-contents)
  - [Quickstart](#quickstart)
  - [Configuration](#configuration)
    - [Configuration file format](#configuration-file-format)

## Quickstart

Currently `bell` is not packaged in any distribution.
To install it, you must compile it from source, and install it with `cargo`'s commands:

```bash
cargo build [--release]
[sudo] cargo install 
```

After that, you should have a `bell` binary installed in your system.
Running it with no arguments will start the application, which will run forever until receiving SIGINT (Ctrl+C).

## Configuration

This application makes use of a [TOML](https://toml.io/en/) file for configuration.
This file in searched in the following standard locations, in this order:

- `$XDG_CONFIG_HOME/bell/config.toml` - XDG user configuration
- `$HOME/.config/bell/config.toml` - Linux-general user configuration
- `/usr/local/share/bell/config.toml` - Global configuration

### Configuration file format

The following is a sample configuration file detailing some configuration options available.

```toml
# Global options are specified without a group.
# They apply to all outputs unless they are overriden.
#
# All options valid for a single output are also valid for the global configuration.
#
# Colors are always specified in 0xAARRGGBB format.

# Size in pixels of the notification surface
width = 300
height = 120

# Layout of the notification message.
#
# All text can be customized by the <(option)=(value) ...> syntax.
# Available options currently are:
#   font_size: Font size in pixels
#   color: Text foreground color in 0xAARRGGBB format
#
# Besides that, the following texts are substituted by the notification parameter:
#   app_name: Name of the application that triggered the notification (if set).
#   summary: Short text describing the notification.
#   body: Full text body of the notification.
message_layout = """
<font_size=18.0 summary>
from <app_name>

<color=0x80FFFFFF ghost text>

<font_size=12.0 body>
"""

# Icon theme to use when trying to render icons.
icon_theme = "Adwaita"

# Special section for describing actions to perform upon interacting with the notification.
#
# The available triggers are:
#   left-click, right-click, middle-click
#
# The available actions are:
#   nothing: Do nothing. The default when no action is configured for a trigger.
#   close-notification: Close all surfaces of the current notification.
[events]
right-click = "close-notification"

# Specify per-input options.
[outputs."eDP-1"]
background_color = 0xFF406060

border_color = 0xFF000000
border_size = 2
border_radius = 0

# Where to anchor the notification on the output.
#
# Available options are:
#   Top, Bottom, Left, Right
#
# A combination of them (using '|') specifies an output corner.
anchor = "Right | Bottom"
# Direction on which notifications will stack if there's multiple of them.
# (e.g. 'Up' means that new notifications will push the old ones to above them).
#
# Available options are:
#   Up, Down, Left, Right
direction = "Up"

# Layer on which to display the notification surfaces.
#
# Available options are:
#   Background, Bottom, Top, Overlay
#
# For more information, see
# https://wayland.app/protocols/wlr-layer-shell-unstable-v1#zwlr_layer_shell_v1:enum:layer
layer = "Overlay"

# Margins from one notification to another and from the output borders.
#
# For instance, the following margins specify that notifications will have a
# spacing of 2 pixels from all borders of the screen, and between notifications,
# 4 pixels are expected (2 from each one's margins).
[outputs."eDP-1".margins]
top = 2
right = 2
bottom = 2
left = 2
```
