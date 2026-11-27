creabim_mcp_python() {
    for candidate in "${CREABIM_MCP_PYTHON:-}" "$(command -v python3.12 || true)" \
        /opt/homebrew/opt/python@3.12/bin/python3.12 \
        /usr/local/opt/python@3.12/bin/python3.12 \
        /Library/Frameworks/Python.framework/Versions/3.12/bin/python3.12; do
        if [ -x "$candidate" ] && "$candidate" -I -S -c \
            'import sys; sys.exit(sys.implementation.name != "cpython" or sys.version_info[:2] != (3, 12))'; then
            printf '%s\n' "$candidate"
            return
        fi
    done
    echo 'Install CPython 3.12 for this Mac or set CREABIM_MCP_PYTHON to its executable.' >&2
    return 1
}

creabim_mcp_port() {
    case "$1" in ''|*[!0-9]*) return 1 ;; esac
    [ "${#1}" -le 5 ] && [ "$1" -ge 1 ] && [ "$1" -le 65535 ]
}

creabim_mcp_environment() {
    beta_support=${CREABIM_BETA_SUPPORT:-$HOME/Library/Application Support/CreaBIM MCP Beta}
    beta_config="$beta_support/bridge.conf"
    export APPDATA="$beta_support" XDG_CONFIG_HOME="$beta_support" CREABIM_LOG_DIR="$beta_support/Logs"
    unset CREABIM_MCP_PORT CREABIM_MCP_TOKEN CREABIM_MCP_TOKEN_FILE
    if [ ! -e "$beta_config" ] && [ ! -L "$beta_config" ]; then
        return 0
    fi

    if [ -L "$beta_config" ] || [ ! -f "$beta_config" ] || [ ! -O "$beta_config" ] ||
        [ "$(/usr/bin/stat -f %Lp "$beta_config")" != 600 ] ||
        ! IFS=' ' read -r CREABIM_MCP_PORT CREABIM_MCP_TOKEN < "$beta_config"; then
        echo 'CreaBIM MCP Beta requires a private bridge.conf file owned by the current user (mode 0600).' >&2
        return 1
    fi
    case "$CREABIM_MCP_TOKEN" in
        ''|*[!a-f0-9]*) echo 'Invalid CreaBIM MCP Beta bridge token.' >&2; return 1 ;;
    esac
    if [ "${#CREABIM_MCP_TOKEN}" -ne 64 ] || ! creabim_mcp_port "$CREABIM_MCP_PORT"; then
        echo 'Invalid CreaBIM MCP Beta bridge configuration.' >&2
        return 1
    fi
    export CREABIM_MCP_PORT CREABIM_MCP_TOKEN
}
