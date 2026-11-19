#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 1 ]]; then
  echo "Usage: $0 IMAGE" >&2
  exit 2
fi
image=$1
[[ "$(docker image inspect --format '{{.Config.User}}' "$image")" == pgex ]]
docker run --rm --read-only --cap-drop=ALL --security-opt=no-new-privileges "$image" >/dev/null
docker run --rm --entrypoint /bin/sh "$image" -c \
  'test "$(id -u)" != 0 && test -f /usr/share/doc/pgex/LICENSE && test -d /usr/share/doc/pgex/licenses'

work=$(mktemp -d)
network="pgex-image-test-$$-$RANDOM"
database="$network-db"
cleanup() {
  docker rm -f "$database" >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
chmod 755 "$work"
openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 1 \
  -subj /CN=postgres -addext 'subjectAltName=DNS:postgres' \
  -keyout "$work/server.key" -out "$work/server.crt" > "$work/certificate.log" 2>&1
# This is a disposable test key. The server copies it to a postgres-owned 0600 file.
chmod 644 "$work/server.key" "$work/server.crt"
docker network create "$network" >/dev/null
docker run --detach --name "$database" --network "$network" --network-alias postgres \
  --env POSTGRES_PASSWORD=pgex-container-test-only \
  --mount "type=bind,src=$work,dst=/certificates,readonly" \
  postgres:17.11-bookworm sh -c '
    install -o postgres -g postgres -m0600 /certificates/server.key /tmp/pgex-server.key
    exec docker-entrypoint.sh postgres -c ssl=on \
      -c ssl_cert_file=/certificates/server.crt -c ssl_key_file=/tmp/pgex-server.key
  ' >/dev/null
ready=false
for _ in {1..60}; do
  if docker exec "$database" pg_isready -h 127.0.0.1 -U postgres >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 1
done
if [[ "$ready" != true ]]; then
  docker logs "$database" >&2
  exit 1
fi
sql() {
  docker exec -i "$database" psql -X -v ON_ERROR_STOP=1 -U postgres -d "$1" -At
}
sql postgres <<'SQL'
CREATE DATABASE source;
CREATE DATABASE target;
SQL
sql source <<'SQL'
CREATE TABLE public.events (id bigint PRIMARY KEY, tenant_id bigint NOT NULL, payload text NOT NULL);
INSERT INTO public.events VALUES (1,1,'one'),(2,1,'two'),(3,2,'source-other');
SQL
sql target <<'SQL'
CREATE TABLE public.events (id bigint PRIMARY KEY, tenant_id bigint NOT NULL, payload text NOT NULL);
INSERT INTO public.events VALUES (2,1,'old'),(99,1,'obsolete'),(3,2,'keep');
SQL
cat > "$work/pgex.yml" <<'YAML'
source:
  connection:
    url: postgresql://postgres@postgres/source
    ca_file: /config/server.crt
  selection:
    tables: [public.events]
    where: "tenant_id = {{tenant:bigint}}"
target:
  connection:
    url: postgresql://postgres@postgres/target
    ca_file: /config/server.crt
params:
  tenant: "1"
YAML
chmod 600 "$work/pgex.yml"
export PGEX_SOURCE_PASSWORD=pgex-container-test-only
export PGEX_TARGET_PASSWORD=pgex-container-test-only
transfer() {
  docker run --rm --read-only --cap-drop=ALL --security-opt=no-new-privileges \
    --network "$network" --user "$(id -u):$(id -g)" \
    --mount "type=bind,src=$work,dst=/config,readonly" --workdir /config \
    --env PGEX_SOURCE_PASSWORD --env PGEX_TARGET_PASSWORD \
    "$image" --config pgex.yml
}
transfer
expected='1:one,2:two,3:keep'
[[ "$(sql target <<<"SELECT string_agg(id::text || ':' || payload, ',' ORDER BY id) FROM events;")" == "$expected" ]]

# A COPY failure must restore the rows deleted in the same transaction.
sql target <<'SQL'
ALTER TABLE public.events ADD CONSTRAINT fail_copy CHECK (payload <> 'two') NOT VALID;
SQL
if transfer > "$work/failed-copy.log" 2>&1; then
  echo 'The container failed to report the COPY constraint error' >&2
  exit 1
fi
grep -F fail_copy "$work/failed-copy.log"
[[ "$(sql target <<<"SELECT string_agg(id::text || ':' || payload, ',' ORDER BY id) FROM events;")" == "$expected" ]]
printf 'PASS: non-root image, read-only runtime, private YAML mount, verified TLS, parameter filtering, row refresh, and COPY rollback\n'
