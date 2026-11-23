# Fonts

Every `.ttf` / `.otf` file in this folder is loaded when the GPUI window
starts (`ui::load_fonts`). The app's font is the `font` design token
(default `Amarante`, changeable on the UI page). If that family is not
installed and not in this folder, the window falls back to the system font
and the UI page says so.

Amarante is an OFL font from Google Fonts. Put `Amarante-Regular.ttf` here.
