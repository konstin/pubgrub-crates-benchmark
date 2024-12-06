use std::{sync::mpsc, thread, time::Instant};

use crossbeam::channel::unbounded;

use benchmark_from_crates::{
    index_data, process_crate_version, read_index::read_index, Index, Mode, OutputSummary,
};
use clap::Parser;
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator as _, ParallelIterator as _};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Parser, Debug)]
#[command(about, long_about = None)]
struct Args {
    /// Dont filter out core elements of the Solana ecosystem
    #[clap(long)]
    with_solana: bool,

    #[arg(long, short, value_enum, default_value_t = Mode::All)]
    mode: Mode,

    /// Sets the number of threads to be used in the rayon threadpool.
    #[clap(long, short, default_value_t = 0)]
    threads: usize,

    /// Filter to only process crates with a name that contains this string.
    #[clap(long)]
    filter: Option<String>,
}

fn main() {
    let args = Args::parse();
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build_global()
        .unwrap();

    println!(
        "Running in mode {:?} on {} rayon threads.",
        &args.mode,
        rayon::current_num_threads()
    );
    let create_filter = if args.with_solana {
        |_name: &str| true
    } else {
        println!("!!!!!!!!!! Excluding Solana Crates !!!!!!!!!!");
        |name: &str| !name.contains("solana")
    };
    let version_filter = |version: &index_data::Version| !version.yanked;
    println!("!!!!!!!!!! Excluding Yanked !!!!!!!!!!");

    let index =
        crates_index::GitIndex::with_path("index", "https://github.com/rust-lang/crates.io-index")
            .unwrap();
    let index_commit = index.changes().unwrap().next().unwrap().unwrap();
    let data = read_index(&index, create_filter, version_filter);

    let to_prosses: Vec<_> = data
        .par_iter()
        .filter(|(c, _)| args.filter.as_ref().map_or(true, |f| c.contains(f)))
        .flat_map(|(c, v)| v.par_iter().map(|(v, _)| (c.clone(), v)))
        .collect();

    thread::scope(|s| {
        let (out_tx, out_rx) = mpsc::channel::<OutputSummary>();
        let (to_prosses_tx, to_prosses_rx) = unbounded();
        for _ in 0..rayon::current_num_threads() {
            let to_prosses_rx = to_prosses_rx.clone();
            let out_tx = out_tx.clone();
            let mut index = Index::new(&data);
            s.spawn(move || {
                for (crt, ver) in to_prosses_rx {
                    out_tx
                        .send(process_crate_version(&mut index, crt, ver, args.mode))
                        .unwrap();
                }
            });
        }
        drop(out_tx);

        let start = Instant::now();
        for (crt, ver) in &to_prosses {
            to_prosses_tx.send((*crt, (*ver).clone())).unwrap()
        }
        drop(to_prosses_tx);

        let template = "PubGrub: [Time: {elapsed}, Rate: {per_sec}, Remaining: {eta}] {wide_bar} {pos:>6}/{len:6}: {percent:>3}%";
        let style = ProgressBar::new(to_prosses.len() as u64)
            .with_style(ProgressStyle::with_template(template).unwrap())
            .with_finish(ProgressFinish::AndLeave);
        style.set_length(to_prosses.len() as _);

        let mut file_name = "out".to_string();
        if args.with_solana {
            file_name += "_with_solana";
        }
        if let Some(f) = args.filter {
            file_name += "_filtered_to_";
            file_name += &f;
        }
        file_name += "_index_hash_";
        file_name += &index_commit.commit_hex()[..4];
        file_name += ".csv";

        let mut out_file = csv::Writer::from_path(&file_name).unwrap();
        let mut pub_cpu_time = 0.0;
        let mut cargo_cpu_time = 0.0;
        let mut cargo_pub_lock_cpu_time = 0.0;
        let mut pub_cargo_lock_cpu_time = 0.0;
        for row in out_rx {
            style.inc(1);
            pub_cpu_time += row.time;
            cargo_cpu_time += row.cargo_time;
            cargo_pub_lock_cpu_time += row.cargo_check_pub_lock_time;
            pub_cargo_lock_cpu_time += row.pub_check_cargo_lock_time;
            out_file.serialize(row).unwrap();
        }
        let wall_time = start.elapsed().as_secs_f32();
        out_file.flush().unwrap();
        style.finish();

        println!("!!!!!!!!!! Timings !!!!!!!!!!");
        let p = |n: &str, t: f32| {
            if t > 0.0 {
                println!("{n:>20} time: {:>8.2}s == {:>6.2}min", t, t / 60.0)
            } else {
                println!("{n:>20} time: skipped")
            }
        };
        println!("        index commit hash: {}", index_commit.commit_hex());
        println!(
            "        index commit time: {}",
            OffsetDateTime::from(index_commit.time())
                .format(&Rfc3339)
                .unwrap()
        );
        p("Pub CPU", pub_cpu_time);
        p("Cargo CPU", cargo_cpu_time);
        p("Cargo check lock CPU", cargo_pub_lock_cpu_time);
        p("Pub check lock CPU", pub_cargo_lock_cpu_time);
        p("Wall", wall_time);
    });
}
