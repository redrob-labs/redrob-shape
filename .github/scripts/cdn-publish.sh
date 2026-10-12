#!/usr/bin/env bash
#
# Publish one job's staged bundles to https://cdn.redrob.ai/<prefix>/.
#
# COPIED VERBATIM from redrob-design, deliberately. This organisation's publish convention is already decided and
# already in production -- the same host, the same REDROB_CDN_* variables, the same versioned-then-aliased
# ordering, the same gate when the bucket is absent. Rewriting it here in our own style would create a second
# convention to keep in step with the first, and the first is the one the Console product page already links
# against. Diverge from this file only for a reason that applies to browsers and not to the other products, and
# say so in the diff when you do.
#
# The apt REPOSITORY is published by cdn-publish-apt.sh instead: a repository is a tree at a fixed URL, and this
# script's whole model is a flat file with a version in its name.
#
# Two phases, in this order, because a link a page hands out must never resolve
# before the bytes behind it are known to be retrievable: every file goes up
# under <version>/ with its own checksum sidecar and is read back through
# CloudFront and compared, and only then does the version-free alias under
# latest/ move. The alias drops the "-<version>" segment on purpose, so a page
# links a name that does not go stale on the next release.
#
# The bucket and the credentials are the switch. Without them this says which
# variable is missing and exits 0: a CDN nobody has provisioned must not redden
# a desktop build. There is deliberately no GitHub Releases fallback, since a
# 404 from a host we guessed at is worse than an honest absence.
#
# Usage: cdn-publish.sh <staging-dir>
# Environment: APP_VERSION, CDN_PREFIX, CDN_PUBLIC_HOST, REDROB_CDN_BUCKET,
#              AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY
set -euo pipefail

staging="${1:?a staging directory is required}"
version="${APP_VERSION:?APP_VERSION is required}"
prefix="${CDN_PREFIX:?CDN_PREFIX is required}"
host="${CDN_PUBLIC_HOST:?CDN_PUBLIC_HOST is required}"

if [ -z "${REDROB_CDN_BUCKET:-}" ]; then
  echo "::notice::REDROB_CDN_BUCKET is not set, so nothing was uploaded."
  exit 0
fi
if [ -z "${AWS_ACCESS_KEY_ID:-}" ] || [ -z "${AWS_SECRET_ACCESS_KEY:-}" ]; then
  echo "::notice::CDN credentials are not configured, so nothing was uploaded."
  exit 0
fi

content_type() {
  case "$1" in
  *.sha256) echo "text/plain" ;;
  *.exe) echo "application/x-msdownload" ;;
  *.deb) echo "application/vnd.debian.binary-package" ;;
  *.rpm) echo "application/x-rpm" ;;
  *.AppImage) echo "application/x-iso9660-appimage" ;;
  *.dmg) echo "application/x-apple-diskimage" ;;
  *) echo "application/octet-stream" ;;
  esac
}

put() {
  aws s3 cp "$1" "s3://${REDROB_CDN_BUCKET}/${prefix}/$2" \
    --content-type "$(content_type "$2")" \
    --cache-control "$3" \
    --no-progress
}

for file in "$staging"/*; do
  name="$(basename "$file")"
  # Sidecars are written here as we go; only the artefacts are inputs.
  case "$name" in *.sha256) continue ;; esac
  # The alias a page links: the same name with the version taken out of it.
  stable="${name/-$version/}"
  if [ "$stable" = "$name" ]; then
    echo "$name carries no -$version segment, so it has no latest/ alias." >&2
    exit 1
  fi

  (cd "$staging" && sha256sum "$name" >"$name.sha256")
  local_sum="$(sha256sum "$file" | awk '{print $1}')"

  # A versioned key is cached as immutable, so it is written once. These builds
  # are not reproducible byte for byte, so a rerun must not hand two different
  # files to readers who both kept the same URL. When the version is already
  # published, that copy stays authoritative and becomes what the alias mirrors:
  # a rerun still repairs an alias that never moved, and changing what a version
  # means still requires a new version.
  #
  # A missing object is 403 from this CloudFront rather than 404, so the status
  # code is read instead of letting `--fail` redden a first publish.
  published="$RUNNER_TEMP/published-$name"
  published_code="$(curl --location --silent --output "$published" \
    --write-out '%{http_code}' "https://${host}/${prefix}/${version}/${name}" || true)"
  case "$published_code" in
  200)
    published_sum="$(sha256sum "$published" | awk '{print $1}')"
    if [ "$published_sum" = "$local_sum" ]; then
      echo "Already published, unchanged: https://${host}/${prefix}/${version}/${name}"
    else
      echo "::notice::${prefix}/${version}/${name} is already published (published=$published_sum built=$local_sum). Keeping the published bytes; the alias will mirror them."
      file="$published"
      local_sum="$published_sum"
    fi
    ;;
  403 | 404)
    put "$file" "$version/$name" "public, max-age=31536000, immutable"
    put "$file.sha256" "$version/$name.sha256" "public, max-age=31536000, immutable"

    # CloudFront caches the 403 this key just returned, so the read back has to
    # outlast that negative TTL rather than fail on the first attempt.
    remote="$RUNNER_TEMP/verify-$name"
    curl --fail --location --silent --show-error --retry 10 --retry-delay 6 \
      --retry-all-errors -o "$remote" "https://${host}/${prefix}/${version}/${name}"
    remote_sum="$(sha256sum "$remote" | awk '{print $1}')"
    if [ "$local_sum" != "$remote_sum" ]; then
      echo "Checksum mismatch for $name: local=$local_sum remote=$remote_sum" >&2
      exit 1
    fi
    echo "OK https://${host}/${prefix}/${version}/${name} ($remote_sum)"
    ;;
  *)
    echo "Could not check ${prefix}/${version}/${name}: HTTP ${published_code}" >&2
    exit 1
    ;;
  esac

  # The alias moves only now that the versioned copy is known to be readable.
  # Short cache: this key is rewritten by the next publish.
  printf '%s  %s\n' "$local_sum" "$stable" >"$staging/$stable.sha256"
  put "$file" "latest/$stable" "public, max-age=300"
  put "$staging/$stable.sha256" "latest/$stable.sha256" "public, max-age=300"
  echo "OK https://${host}/${prefix}/latest/${stable}"
done
