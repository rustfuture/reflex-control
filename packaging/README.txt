Reflex Control (reflex)
=======================

A calibrated System-1 control plane for AI agents. This archive holds the
`reflex` command-line tool, built from the tagged release.

Install
-------
Put the `reflex` binary (`reflex.exe` on Windows) in a directory on your PATH,
or use the installer, which does that for you and checks the checksum:

  macOS / Linux:  curl -fsSL https://raw.githubusercontent.com/rustfuture/reflex-control/main/install.sh | sh
  Windows:        irm https://raw.githubusercontent.com/rustfuture/reflex-control/main/install.ps1 | iex

Then, inside the project you want to protect:

  reflex --version
  reflex install       # set up agent hooks (Claude Code, git pre-commit)
  reflex doctor        # check the setup

Verify this download
--------------------
Each archive has a matching .sha256 file on the release page:

  sha256sum -c reflex-<version>-<target>.tar.gz.sha256      (Linux)
  shasum -a 256 -c reflex-<version>-<target>.tar.gz.sha256  (macOS)

Docs and source: https://github.com/rustfuture/reflex-control
License: MIT (see LICENSE)
