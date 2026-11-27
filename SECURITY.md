# Security policy

mbus-kit decodes bytes from meters, meter buses and radio receivers before anything about them is known.
A panic, an unbounded loop or an out-of-bounds read on any input is a security bug.

## Reporting

Please report vulnerabilities privately through
[GitHub security advisories](https://github.com/hupe1980/mbus-kit/security/advisories/new)
rather than a public issue. Include the input bytes (hex) if you can.

## Supported versions

Until 1.0, only the latest release receives fixes.
