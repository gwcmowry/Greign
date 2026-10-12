# Greign's Rawler 0.7.0 patch

This is the crates.io 0.7.0 source, with its original licensing retained.

Canon ColorData version 48 (R7/R10 family): read NormalWhiteLevel at word
0x280 and SpecularWhiteLevel at 0x281. The former specular lookup at 0x282
read LinearityUpperMargin and caused premature highlight reconstruction.
Verified against all eight private R7 fixtures, LibRaw, and ExifTool's
Canon ColorData11 field definitions. No decompression code is changed.

Source: https://raw.githubusercontent.com/exiftool/exiftool/master/lib/Image/ExifTool/Canon.pm
