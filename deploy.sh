#!/usr/bin/env bash
# Déploie Hameaux sur la tour depuis ce dossier.
#
#   HAMEAUX_HOST=moi@tour ./deploy.sh
#
# Prérequis sur la tour : rsync, Docker avec le plugin compose.
#
# Copie les sources (rsync) dans ~/apps/hameaux sur la tour (ou HAMEAUX_REMOTE_DIR,
# relatif au dossier personnel), puis reconstruit et relance le conteneur. L'ancien
# conteneur reçoit SIGTERM et sauvegarde le monde avant de s'arrêter ; le nouveau recharge
# cette sauvegarde depuis ~/apps/hameaux/data, que le déploiement ne touche jamais.
set -euo pipefail

HOST="${HAMEAUX_HOST:?Indiquer la tour : HAMEAUX_HOST=utilisateur@machine ./deploy.sh}"
DIR="${HAMEAUX_REMOTE_DIR:-apps/hameaux}"

cd "$(dirname "$0")"
ssh "$HOST" "mkdir -p '$DIR/data'"
# Les chemins exclus (dont /data et /.env) sont protégés de --delete.
rsync -az --delete \
  --exclude /target --exclude /data --exclude /.git --exclude /.claude \
  --exclude /web/node_modules --exclude /web/dist --exclude /.env \
  ./ "$HOST:$DIR/"

# Premier déploiement : .env à partir de l'exemple, avec l'utilisateur de la tour.
ssh "$HOST" "cd '$DIR' \
  && { [ -f .env ] || { cp .env.example .env && printf 'HAMEAUX_UID=%s\nHAMEAUX_GID=%s\n' \"\$(id -u)\" \"\$(id -g)\" >> .env; }; } \
  && docker compose up -d --build && docker compose ps"
