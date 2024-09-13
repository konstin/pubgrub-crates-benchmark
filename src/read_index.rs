use std::{
    collections::{BTreeMap, HashMap},
    time::Instant,
};

use cargo::{core::Summary, util::interning::InternedString};
use crates_index::GitIndex;
use rayon::iter::ParallelIterator;

use crate::index_data;

pub fn read_index(
    index: &GitIndex,
    create_filter: impl Fn(&str) -> bool + Sync + 'static,
    version_filter: impl Fn(&index_data::Version) -> bool + Sync + 'static,
) -> HashMap<InternedString, BTreeMap<semver::Version, (index_data::Version, Summary)>> {
    println!("Start reading index");
    let start = Instant::now();
    let crates: HashMap<InternedString, BTreeMap<semver::Version, (index_data::Version, Summary)>> =
        index
            .crates_parallel()
            .map(|c| c.unwrap())
            .filter(|crt| create_filter(crt.name()))
            .map(|crt| {
                let name: InternedString = crt.name().into();
                let ver_lookup = crt
                    .versions()
                    .iter()
                    .filter_map(|v| TryInto::<index_data::Version>::try_into(v).ok())
                    .filter(|v| version_filter(v))
                    .filter_map(|v| {
                        let s: Summary = (&v).try_into().ok()?;

                        Some(((*v.vers).clone(), (v, s)))
                    })
                    .collect();
                (name, ver_lookup)
            })
            .collect();
    println!(
        "Done reading index in {:.1}s",
        start.elapsed().as_secs_f32()
    );
    crates
}

#[cfg(test)]
pub fn read_test_file(
    iter: impl IntoIterator<Item = index_data::Version>,
) -> HashMap<InternedString, BTreeMap<semver::Version, (index_data::Version, Summary)>> {
    let mut deps: HashMap<
        InternedString,
        BTreeMap<semver::Version, (index_data::Version, Summary)>,
    > = HashMap::new();

    for v in iter {
        let s = (&v).try_into().unwrap();
        deps.entry(v.name.clone())
            .or_default()
            .insert((*v.vers).clone(), (v, s));
    }

    deps
}
