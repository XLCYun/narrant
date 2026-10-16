# Use OSS as the Stable Application Update authority

Status: Superseded by [ADR-0036](0036-use-one-public-github-repository-for-distribution.md) for repository and distribution authority. The following records the historical decision.


Status: Superseded by ADR-0036. Supersedes [ADR-0028](0028-deliver-staged-application-updates-from-public-releases.md) only where it selected GitHub Releases as the Stable channel; the staged client lifecycle and trust-root decisions remain in force.

Narrant clients and the CY Lab website read only
`https://updates.cyunlab.com/narrant/channels/stable/latest.json`. Each manifest target has a content-addressed OSS updater `url` and a human-facing OSS `installer_url`. GitHub Releases continue to contain the same eight public artifacts as a release archive, but GitHub Latest is not a client or website update authority. Existing clients on the former GitHub endpoint are not bridged.

Promotion uploads and byte-verifies all immutable release objects and an isolated candidate manifest before publishing the fixed GitHub Draft. After GitHub confirms that Release as Latest, promotion acquires the append-only OSS lock, rechecks the previous Stable identity bound to native smoke evidence, replaces the Stable manifest last, reads it back byte-for-byte, and releases the exact lock owner. A mismatch leaves Stable unchanged and the lock available for investigation according to the runbook.

The first OSS Stable uses one protected exact `NARRANT_OSS_BOOTSTRAP_TAG`. It is allowed only when the Stable object is genuinely absent; an existing object or any ambiguous read failure rejects bootstrap. Later promotions require the current Stable version and raw SHA-256 to match the evidence inputs. OSS Versioning provides recovery for the mutable pointer, while release object keys remain immutable and the publishing role has delete permission only for its promotion lock.

Anonymous bucket policy exposes only Narrant release objects, candidate manifests, and the Stable manifest. GitHub Actions obtains a short-lived, repository-and-environment-bound Alibaba Cloud OIDC session through a Narrant-specific least-privilege RAM role. The website uses an exact-origin `https://cyunlab.com` GET/HEAD CORS rule.
