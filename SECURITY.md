# Security policy

Retort runs FFmpeg on files you open, so bugs that let a crafted file or file
name do more than fail a conversion are security issues — for example
anything that makes Retort run something other than FFmpeg, write outside the
output folder, or overwrite a file without being asked.

## Reporting

Please report privately through GitHub: *Security → Report a vulnerability*
on the repository. Do not open a public issue. You will get an answer within
a week; fixes are released as soon as they are ready and credited unless you
prefer otherwise.

Vulnerabilities in FFmpeg itself should go to the FFmpeg project; tell us too
if the Flatpak or AppImage (which bundle FFmpeg) need a rebuild.

## Supported versions

Only the latest release receives fixes.
