# dmgbuild settings for the Scorecard macOS disk image.
# Built with dmgbuild==1.6.5: dmgbuild -s packaging/dmg-settings.py
# "Scorecard $ver" dist/sc-....dmg
# Icon positions are fixed Finder content coordinates. The installer
# script sits prominent at left center with INSTALL.txt beside it;
# the binaries, README, and LICENSE form a lower row. No
# /Applications link: these are CLI tools, dragging them to
# Applications would not put them on PATH.

# Passed on the dmgbuild command line (-D stage=... -D packaging=...),
# so the settings work no matter which directory invokes the build
# (dmgbuild execs this file without __file__; -D values land in the
# `defines` dict, not as bare names).
import os

stage = defines["stage"]
packaging = defines["packaging"]

format = "UDZO"
filesystem = "HFS+"
size = None

files = [os.path.join(stage, name) for name in (
    "Install Scorecard.command",
    "INSTALL.txt",
    "sc",
    "sc-mcp",
    "README.md",
    "LICENSE",
)]

icon_size = 80
text_size = 12

window_rect = ((200, 120), (600, 400))

background = os.path.join(packaging, "dmg-background.png")

show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

icon_locations = {
    "Install Scorecard.command": (150, 175),
    "INSTALL.txt": (320, 175),
    "sc": (90, 285),
    "sc-mcp": (230, 285),
    "README.md": (370, 285),
    "LICENSE": (510, 285),
}
