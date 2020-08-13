pub mod calc_spill_weight;
pub mod inst;
pub mod pro_epi_inserter;
pub mod reg_coalescer;
pub mod regalloc;
pub mod replace_copy;
pub mod validate_frame_index;
// pub mod replace_data;
pub mod inst_def;
pub mod live_interval_splitter;
pub mod register;
pub mod spiller;
pub use super::frame_object;
