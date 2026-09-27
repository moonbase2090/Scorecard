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
    "Install Scorecard.pkg",
    "INSTALL.txt",
    "sc",
    "sc-mcp",
    "README.md",
    # LICENSE ships with a .txt suffix so Finder shows a text icon
    # instead of the generic '?' document.
    "LICENSE.txt",
)]

icon_size = 96
text_size = 13

window_rect = ((200, 120), (800, 560))

background = os.path.join(packaging, "dmg-background.png")

show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

icon_locations = {
    "Install Scorecard.pkg": (330, 210),
    "INSTALL.txt": (470, 210),
    "sc": (130, 368),
    "sc-mcp": (310, 368),
    "README.md": (490, 368),
    "LICENSE.txt": (670, 368),
}
