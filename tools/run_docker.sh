#!/usr/bin/env bash
#
# run-pgtrino-gateway.sh
# 啟動 PostgreSQL-Trino Gateway container。
#
# 不帶任何參數執行 -> 顯示使用說明（不會啟動 container）
# 帶 -y / --yes 才會真正執行 docker run（避免誤用預設值啟動）
#
set -euo pipefail

# ----------------------------
# 預設值（對應 Configuration 表格）
# ----------------------------
IMAGE_NAME="pgtrino-gateway:v1.0"
CONTAINER_NAME="pgtrino-gateway"
HOST_PORT="5432"

LISTEN_ADDR="127.0.0.1:5432"
TRINO_HOST="localhost"
TRINO_PORT="8080"
TRINO_CATALOG="memory"
TRINO_SCHEMA="default"
TRINO_USER="trino"

TRINO_SSL="off"
TRINO_TLS_NO_VERIFY="off"
TRINO_ALLOW_PLAINTEXT_AUTH="off"

AUTH="off"
ALLOW_INSECURE_LISTENER="off"

MAX_CONNECTIONS="256"

TLS_CERT_PATH=""
TLS_KEY_PATH=""

LOG_LEVEL=""          # 對應 RUST_LOG=postgresql_trino_gateway=<level>
DRAIN_TIMEOUT_SECS="" # 對應 GATEWAY_SHUTDOWN_DRAIN_TIMEOUT_SECS

CONFIRM_RUN="false"

# ----------------------------
# 使用說明
# ----------------------------
usage() {
cat <<'HELP'
用法:
  ./run-pgtrino-gateway.sh [選項...] -y

  未帶任何參數時只會顯示這份說明，不會啟動 container。
  確認參數無誤後，加上 -y 或 --yes 才會實際執行 docker run。

Gateway 參數（對應 PostgreSQL-Trino Gateway 的 Configuration）:

  --listen-addr <addr:port>       PG 監聽位址
                                   預設: 127.0.0.1:5432
                                   若要讓 docker -p 對外映射連進來，
                                   通常要改成 0.0.0.0:5432

  --trino-host <host>             Trino server 主機名稱
                                   預設: localhost

  --trino-port <port>             Trino server port
                                   預設: 8080

  --trino-catalog <catalog>       預設 Trino catalog
                                   預設: memory

  --trino-schema <schema>         預設 Trino schema
                                   預設: default

  --trino-user <user>             --auth=false 時，代表 PG client 的 Trino 使用者
                                   預設: trino

  --trino-ssl                     對 Trino 的請求使用 HTTPS
                                   預設: off（不加此參數即為關閉）

  --trino-tls-no-verify           跳過 Trino TLS 憑證驗證
                                   預設: off
                                   （通常搭配自簽憑證測試環境使用，正式環境不建議開）

  --trino-allow-plaintext-auth    允許將帳密以 plaintext 方式透過 HTTP 轉送給 Trino
                                   預設: off
                                   （沒有 HTTPS 時開啟此項會讓密碼以明文傳輸，僅建議測試環境使用）

  --auth                          要求 PG client 端輸入密碼
                                   預設: off

  --allow-insecure-listener        允許 --auth=off 且 --listen-addr 非 loopback 的組合
                                   預設: off
                                   （若監聽位址非 127.0.0.1 且未開啟 --auth，通常需要加這個參數才能啟動）

  --max-connections <數字>         最大同時連線數
                                   預設: 256

  --tls-cert <host路徑>            PG 監聽端的 TLS 憑證鏈 (PEM)，需與 --tls-key 一併提供
                                   預設: 無（不啟用 TLS）

  --tls-key <host路徑>             PG 監聽端的 TLS 私鑰 (PEM)，需與 --tls-cert 一併提供
                                   預設: 無（不啟用 TLS）

  --log-level <level>              除錯用，對應官方文件的 RUST_LOG。
                                   Gateway 對 Trino 的連線是「per-query 觸發」，
                                   預設 log 只記錄連線事件，不會顯示查詢/轉發內容。
                                   可選值: debug（記錄每個查詢與 rewrite）、
                                          trace（額外記錄 protocol 層級決策）
                                   預設: 不設定（等同官方預設 log level）
                                   注意: debug/trace 會把 SQL 內容（含常數值）寫進 log，
                                        若查詢含 PII 請注意 log 存放與保留政策。

  --drain-timeout <秒數>           SIGTERM/SIGINT 時的連線 drain 時間
                                   對應 GATEWAY_SHUTDOWN_DRAIN_TIMEOUT_SECS
                                   預設: 不設定（gateway 內建預設 25 秒）

Docker 執行相關參數:

  --image <image:tag>             要執行的 image
                                   預設: pgtrino-gateway:v1.0

  --name <container名稱>          container 名稱
                                   預設: pgtrino-gateway

  --host-port <port>               對外映射的 host port（容器內部固定監聽 5432）
                                   預設: 5432

  -y, --yes                        實際執行 docker run（不加此項只會印出將要執行的指令並結束）

  -h, --help                       顯示此說明

範例:

  1) 先看看預設值會產生什麼指令（不執行）:
     ./run-pgtrino-gateway.sh

  2) 本機測試，用 memory catalog，不驗證密碼:
     ./run-pgtrino-gateway.sh --listen-addr 0.0.0.0:5432 -y

  3) 接遠端 Trino，開 SSL，要求 PG client 密碼:
     ./run-pgtrino-gateway.sh \
       --listen-addr 0.0.0.0:5432 \
       --trino-host trino.example.com \
       --trino-port 8443 \
       --trino-ssl \
       --trino-catalog iceberg \
       --trino-schema default \
       --auth \
       -y

  4) 開啟 PG 監聽端 TLS:
     ./run-pgtrino-gateway.sh \
       --listen-addr 0.0.0.0:5432 \
       --tls-cert /etc/pgtrino/certs/server.crt \
       --tls-key /etc/pgtrino/certs/server.key \
       -y

  5) 除錯：確認 Gateway 是否真的把查詢轉發給 Trino
     （官方文件說明 Gateway 對 Trino 是 per-query 觸發連線，
      預設 log level 不會顯示查詢/轉發內容，需開 debug）:
     ./run-pgtrino-gateway.sh \
       --listen-addr 0.0.0.0:5432 \
       --trino-host 10.4.2.56 \
       --trino-port 8443 \
       --trino-ssl \
       --trino-tls-no-verify \
       --auth \
       --log-level debug \
       -y
     # 啟動後用 psql 連進去並執行一個查詢，再看:
     #   docker logs -f pgtrino-gateway
     # 應該會看到查詢文字與是否成功轉發給 Trino 的訊息。
HELP
}

# ----------------------------
# 沒有任何參數 -> 顯示說明並結束
# ----------------------------
if [[ $# -eq 0 ]]; then
  usage
  exit 0
fi

# ----------------------------
# 解析參數
# ----------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --listen-addr) LISTEN_ADDR="$2"; shift 2 ;;
    --trino-host) TRINO_HOST="$2"; shift 2 ;;
    --trino-port) TRINO_PORT="$2"; shift 2 ;;
    --trino-catalog) TRINO_CATALOG="$2"; shift 2 ;;
    --trino-schema) TRINO_SCHEMA="$2"; shift 2 ;;
    --trino-user) TRINO_USER="$2"; shift 2 ;;
    --trino-ssl) TRINO_SSL="on"; shift ;;
    --trino-tls-no-verify) TRINO_TLS_NO_VERIFY="on"; shift ;;
    --trino-allow-plaintext-auth) TRINO_ALLOW_PLAINTEXT_AUTH="on"; shift ;;
    --auth) AUTH="on"; shift ;;
    --allow-insecure-listener) ALLOW_INSECURE_LISTENER="on"; shift ;;
    --max-connections) MAX_CONNECTIONS="$2"; shift 2 ;;
    --tls-cert) TLS_CERT_PATH="$2"; shift 2 ;;
    --tls-key) TLS_KEY_PATH="$2"; shift 2 ;;
    --log-level) LOG_LEVEL="$2"; shift 2 ;;
    --drain-timeout) DRAIN_TIMEOUT_SECS="$2"; shift 2 ;;
    --image) IMAGE_NAME="$2"; shift 2 ;;
    --name) CONTAINER_NAME="$2"; shift 2 ;;
    --host-port) HOST_PORT="$2"; shift 2 ;;
    -y|--yes) CONFIRM_RUN="true"; shift ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "未知參數: $1" >&2
      echo "使用 -h 或 --help 查看使用說明" >&2
      exit 1
      ;;
  esac
done

# ----------------------------
# 組裝 docker run 參數
# ----------------------------
DOCKER_ARGS=(
  run -d
  --name "${CONTAINER_NAME}"
  -p "${HOST_PORT}:5432"
)

if [[ -n "${LOG_LEVEL}" ]]; then
  DOCKER_ARGS+=(-e "RUST_LOG=postgresql_trino_gateway=${LOG_LEVEL}")
fi
if [[ -n "${DRAIN_TIMEOUT_SECS}" ]]; then
  DOCKER_ARGS+=(-e "GATEWAY_SHUTDOWN_DRAIN_TIMEOUT_SECS=${DRAIN_TIMEOUT_SECS}")
fi

GATEWAY_ARGS=(
  --listen-addr "${LISTEN_ADDR}"
  --trino-host "${TRINO_HOST}"
  --trino-port "${TRINO_PORT}"
  --trino-catalog "${TRINO_CATALOG}"
  --trino-schema "${TRINO_SCHEMA}"
  --trino-user "${TRINO_USER}"
  --max-connections "${MAX_CONNECTIONS}"
)

[[ "${TRINO_SSL}" == "on" ]] && GATEWAY_ARGS+=(--trino-ssl)
[[ "${TRINO_TLS_NO_VERIFY}" == "on" ]] && GATEWAY_ARGS+=(--trino-tls-no-verify)
[[ "${TRINO_ALLOW_PLAINTEXT_AUTH}" == "on" ]] && GATEWAY_ARGS+=(--trino-allow-plaintext-auth)
[[ "${AUTH}" == "on" ]] && GATEWAY_ARGS+=(--auth)
[[ "${ALLOW_INSECURE_LISTENER}" == "on" ]] && GATEWAY_ARGS+=(--allow-insecure-listener)

if [[ -n "${TLS_CERT_PATH}" && -n "${TLS_KEY_PATH}" ]]; then
  if [[ ! -f "${TLS_CERT_PATH}" || ! -f "${TLS_KEY_PATH}" ]]; then
    echo "錯誤：--tls-cert 或 --tls-key 指定的檔案不存在" >&2
    exit 1
  fi
  # docker -v 要求來源是絕對路徑，否則會被當成 named volume 名稱，
  # 導致容器內對應路徑變成空目錄而非檔案。這裡自動轉成絕對路徑。
  TLS_CERT_PATH="$(realpath "${TLS_CERT_PATH}")"
  TLS_KEY_PATH="$(realpath "${TLS_KEY_PATH}")"
  DOCKER_ARGS+=(
    -v "${TLS_CERT_PATH}:/stackable/certs/server.crt:ro"
    -v "${TLS_KEY_PATH}:/stackable/certs/server.key:ro"
  )
  GATEWAY_ARGS+=(
    --tls-cert /stackable/certs/server.crt
    --tls-key /stackable/certs/server.key
  )
elif [[ -n "${TLS_CERT_PATH}" || -n "${TLS_KEY_PATH}" ]]; then
  echo "錯誤：--tls-cert 與 --tls-key 必須同時提供" >&2
  exit 1
fi

# ----------------------------
# Listener 端 auth/TLS 政策檢查（依官方 README 政策矩陣）
# ----------------------------
IS_LOOPBACK="false"
[[ "${LISTEN_ADDR}" == 127.0.0.1:* ]] && IS_LOOPBACK="true"

HAS_LISTENER_TLS="false"
[[ -n "${TLS_CERT_PATH}" && -n "${TLS_KEY_PATH}" ]] && HAS_LISTENER_TLS="true"

if [[ "${AUTH}" != "on" ]]; then
  if [[ "${IS_LOOPBACK}" == "false" && "${ALLOW_INSECURE_LISTENER}" != "on" ]]; then
    echo "錯誤：--auth 未開啟且 --listen-addr 非 loopback（${LISTEN_ADDR}），" >&2
    echo "      官方政策會拒絕啟動，除非加上 --allow-insecure-listener。" >&2
    exit 1
  fi
else
  if [[ "${HAS_LISTENER_TLS}" == "false" ]]; then
    if [[ "${IS_LOOPBACK}" == "true" ]]; then
      echo "提示：--auth 已開啟但未設定 --tls-cert/--tls-key，屬於允許的組合，" >&2
      echo "      但 gateway 啟動時會顯示明文密碼警告（listener 端未加密）。" >&2
    else
      echo "錯誤：--auth 已開啟、未設定 listener TLS（--tls-cert/--tls-key），" >&2
      echo "      且 --listen-addr 非 loopback（${LISTEN_ADDR}）。" >&2
      echo "      官方政策會拒絕此組合（密碼會以明文跨網路傳輸）。" >&2
      exit 1
    fi
  fi
fi

# --auth 開啟且要連 Trino 走明文 HTTP 時，需要 --trino-allow-plaintext-auth
if [[ "${AUTH}" == "on" && "${TRINO_SSL}" != "on" && "${TRINO_ALLOW_PLAINTEXT_AUTH}" != "on" ]]; then
  echo "錯誤：--auth 已開啟，但 Trino 端未開 --trino-ssl，且未加 --trino-allow-plaintext-auth。" >&2
  echo "      官方政策會拒絕啟動（避免密碼以明文送到 Trino）。" >&2
  echo "      請加上 --trino-ssl，或明確加上 --trino-allow-plaintext-auth 承擔風險。" >&2
  exit 1
fi

# ----------------------------
# 印出將執行的指令
# ----------------------------
echo "將執行的指令："
echo "docker ${DOCKER_ARGS[*]} ${IMAGE_NAME} ${GATEWAY_ARGS[*]}"
echo

if [[ "${CONFIRM_RUN}" != "true" ]]; then
  echo "尚未加上 -y / --yes，不會實際執行。"
  echo "確認指令無誤後，請加上 -y 重新執行。"
  exit 0
fi

# ----------------------------
# 移除同名舊容器（若存在）並執行
# ----------------------------
if docker ps -a --format '{{.Names}}' | grep -qx "${CONTAINER_NAME}"; then
  echo "偵測到同名容器 ${CONTAINER_NAME}，先移除..."
  docker rm -f "${CONTAINER_NAME}" >/dev/null
fi

docker "${DOCKER_ARGS[@]}" "${IMAGE_NAME}" "${GATEWAY_ARGS[@]}"

echo
echo "容器已啟動：${CONTAINER_NAME}"
echo "PG 連線位置：localhost:${HOST_PORT}"
echo "查看 log： docker logs -f ${CONTAINER_NAME}"

