// some boot images were shipped with a deployed container hash
// that does not match what was released. This leads Zincati to not
// find the booted deployement in the graph, and cannot update out of it.
// To unstuck these nodes we serve an incorrect graph one day of the week
// to allow these nodes to update.

use chrono::prelude::*;
use commons::metadata::Release;
use failure::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::option::Option;

static BAD_HASHES_SOURCE_PATH: &str = "/data.json";

// This is all strings, so let's define some aliases to make it easier to reason about
type Version = String;
type Arch = String;
type Digest = String;

// Each entry in the good / bad hashes map looks like this:
//   "43.20251024.3.0": {
//        "x86_64": {
//          "good": "sha256:44528ecc3fe8ab2c2a4d0990cdc0898aca334aa632d4a77f23118f8900435636",
//          "bad": "sha256:ca99893c80a7b84dd84d4143bd27538207c2f38ab6647a58d9c8caa251f9a087"
//        },
//        "aarch64": {
//          "good": "sha256:bba9eff19e3da927c09644eefd42303c4dc7401844cee8e849115466f13b08e9",
//          "bad": "sha256:bb356df5b2a9356c0ec966a35abd0cd8b199c8ddc18b9c70e81baa4c2401796c"
//        },
// ..... // the other arches
// }

/// Represents a hash mapping between the good and bad SHA-256 digests.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GoodBadDigests {
    pub good: Digest,
    pub bad: Digest,
}

/// Under each version, there is a bad-good digest map for each architecture
pub type VersionEntry = HashMap<Arch, GoodBadDigests>;

// The top level entry in the map.
/// unfortunately we can't apply derive macros to type aliases
// so we wrap it into the struct then use serde's flatten attribute
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DigestsMapper {
    #[serde(flatten)]
    version_digests_map: HashMap<Version, VersionEntry>,
}

impl DigestsMapper {
    pub fn new_from_file() -> Result<DigestsMapper, Error> {
        let file = File::open(BAD_HASHES_SOURCE_PATH)?;
        let reader = BufReader::new(file);

        let digests = serde_json::from_reader(reader)?;
        Ok(digests)
    }

    // we only inject wrong values every even minute. The graph is
    // reconstructed after cache expiration which is every 30 secs.
    pub fn should_patch(&self) -> bool {
        let now: DateTime<Utc> = Utc::now();
        now.time().minute().is_multiple_of(2)
    }

    fn get_bad_hash_for_version_and_arch(&self, version: &Version, arch: &Arch) -> Option<String> {
        self.version_digests_map
            .get(version)
            .and_then(|version_entry| version_entry.get(arch).map(|digests| digests.bad.clone()))
    }

    pub fn fix_releases(&self, releases: &mut Vec<Release>) {
        // We don't want to touch the last entry, it needs to be a valid target for update.
        let last_release = releases.pop();
        // let's exit early if empty as there is nothing we can do.
        // This avoids wrapping the whole function under a if let...
        if last_release.is_none() {
            return;
        }

        for entry in releases.iter_mut() {
            if let Some(releases_oci) = entry.oci_images.as_mut() {
                // The unwrap is safe here as we checked for is_some() above
                for oci_release in releases_oci.iter_mut() {
                    let bad_hash = self.get_bad_hash_for_version_and_arch(
                        &entry.version,
                        &oci_release.architecture,
                    );

                    if let Some(bad_hash) = bad_hash {
                        debug!(
                            "found bad hash for {} - {}",
                            &entry.version, &oci_release.architecture
                        );
                        debug!("Original ReleaseOciImage:\n {oci_release:?}");
                        // digest_ref is a digested pullspec: $oci_image_name@$digest so we need to split it
                        // and change only the digest part.
                        let (img_name, _) = oci_release
                            .digest_ref
                            .split_once('@')
                            // The unwrap is safe here, we are always dealing with a digested pullspec
                            // properly deserializing this would requires pulling in osree_rs crate, it's not worth it
                            .unwrap();

                        oci_release.digest_ref = format!("{img_name}@{bad_hash}");
                        info!(
                            "Patched release {} with a bad digest from the bootimage.",
                            &entry.version
                        );
                        debug!("Patched ReleaseOciImage:\n {oci_release:?}");
                    }
                }
            }
        }

        // safe unwrap here as the option was checked early on
        releases.push(last_release.unwrap());
    }
}
