CreaBIM MCP Beta for macOS

Drag CreaBIM MCP Beta.app to Applications. It has its own preferences and
logs in ~/Library/Application Support/CreaBIM MCP Beta.

The MCP server needs CPython 3.12 for the architecture of this disk image.
The app includes its Python dependencies and CreaBIM CLI. The launcher finds
python3.12 on PATH, Homebrew, or the python.org framework installation. For
other locations, set CREABIM_MCP_PYTHON to the interpreter's absolute path
in the MCP client's server environment.

Configure your MCP client to launch this command over stdio:

  /Applications/CreaBIM MCP Beta.app/Contents/MacOS/creabim-mcp

The six saved-project tools are available with the app closed. To enable
live tools, run this command in Terminal, then open or restart the Beta:

  '/Applications/CreaBIM MCP Beta.app/Contents/MacOS/creabim-mcp' --enable-live

The command creates a private bridge.conf file with a random token. The
bridge listens on 127.0.0.1:62457 while the Beta runs. Pass a port after
--enable-live if that port is in use. To disable the bridge, quit the Beta
and remove its bridge.conf. Live changes use the app's normal Save and Undo.

This development build has an ad hoc code signature for local testing.

Building from source

Run these commands from the repository root on the Mac architecture you
want to package. CPython 3.12 with pip is also required at build time.

  cargo build --locked --profile fast -p creabim-app -p creabim-cli --features creabim-app/live-mcp
  bash packaging/macos/make-dmg.sh 1.2.0 "$(date -u +%F)" target/fast "CreaBIM-MCP-Beta-$(uname -m).dmg" --mcp-beta

Use a version matching Cargo.toml. The macOS MCP Beta workflow builds the
same disk image and uploads it as a GitHub Actions artifact.
