use std::path::Path;

use reth_chainspec::ChainSpecBuilder;
use reth_db::{mdbx::DatabaseArguments, open_db_read_only, DatabaseEnv};
use reth_provider::{providers::StaticFileProvider, ProviderFactory};

pub fn get_reth_factory(
    db_path: &Path,
    static_files_path: &Path,
) -> anyhow::Result<ProviderFactory<DatabaseEnv>> {
    let db =
        open_db_read_only(db_path, DatabaseArguments::default()).expect("Could not open database");

    let spec = ChainSpecBuilder::mainnet().build();
    let factory: ProviderFactory<reth_db::DatabaseEnv> = ProviderFactory::new(
        db,
        spec.into(),
        StaticFileProvider::read_only(static_files_path).unwrap(),
    );

    Ok(factory)
}
