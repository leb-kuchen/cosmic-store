use appstream::{enums::Icon, Collection};
use cosmic::widget;
use packagekit_zbus::{
    zbus::blocking::Connection, PackageKit::PackageKitProxyBlocking,
    Transaction::TransactionProxyBlocking,
};
use std::{collections::HashMap, error::Error, sync::Arc};

use super::{Backend, Package};
use crate::{get_translatable, AppstreamCache};

pub struct Packagekit {
    connection: Connection,
    appstream_cache: Arc<AppstreamCache>,
    locale: String,
}

impl Packagekit {
    pub fn new(
        appstream_cache: &Arc<AppstreamCache>,
        locale: &str,
    ) -> Result<Self, Box<dyn Error>> {
        //TODO: cache more zbus stuff?
        let connection = Connection::system()?;
        Ok(Self {
            connection,
            appstream_cache: appstream_cache.clone(),
            locale: locale.to_string(),
        })
    }

    fn transaction(&self) -> Result<TransactionProxyBlocking, Box<dyn Error>> {
        //TODO: use async?
        let pk = PackageKitProxyBlocking::new(&self.connection)?;
        //TODO: set locale
        let tx_path = pk.create_transaction()?;
        let tx = TransactionProxyBlocking::builder(&self.connection)
            .destination("org.freedesktop.PackageKit")?
            .path(tx_path)?
            .build()?;
        Ok(tx)
    }
}

impl Backend for Packagekit {
    fn installed(&self) -> Result<Vec<Package>, Box<dyn Error>> {
        let mut package_ids = Vec::new();
        {
            let tx = self.transaction()?;
            let filter_installed = 1 << 2;
            //let filter_application = 1 << 24;
            tx.get_packages(filter_installed)?;
            for signal in tx.receive_all_signals()? {
                match signal.member() {
                    Some(member) => {
                        if member == "Package" {
                            // https://www.freedesktop.org/software/PackageKit/gtk-doc/Transaction.html#Transaction::Package
                            let (_info, package_id, _summary) =
                                signal.body::<(u32, String, String)>()?;
                            package_ids.push(package_id);
                        } else if member == "Finished" {
                            break;
                        } else {
                            println!("unknown signal {}", member);
                        }
                    }
                    None => {}
                }
            }
        }

        let mut packages = Vec::new();
        for package_id in package_ids {
            let mut parts = package_id.split(';');
            let package_name = parts.next().unwrap_or(&package_id);
            let version_opt = parts.next();
            let architecture_opt = parts.next();

            let data = parts.next().unwrap_or("");
            let mut data_parts = data.split(':');
            let status_opt = data_parts.next();
            let origin_opt = data_parts.next();

            match self.appstream_cache.pkgnames.get(package_name) {
                Some(ids) => {
                    for id in ids.iter() {
                        match self.appstream_cache.collections.get(id) {
                            Some(collection) => {
                                for component in collection.components.iter() {
                                    let mut icon_opt = None;
                                    //TODO: find best icon by size
                                    for component_icon in component.icons.iter() {
                                        //TODO: support other types of icons
                                        match component_icon {
                                            Icon::Cached {
                                                path,
                                                width,
                                                height,
                                                scale,
                                            } => {
                                                if let Some(icon_path) = AppstreamCache::icon_path(
                                                    collection.origin.as_deref(),
                                                    path,
                                                    *width,
                                                    *height,
                                                    *scale,
                                                ) {
                                                    icon_opt =
                                                        Some(widget::icon::from_path(icon_path));
                                                    break;
                                                }
                                            }
                                            Icon::Stock(stock) => {
                                                icon_opt = Some(
                                                    widget::icon::from_name(stock.clone()).handle(),
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                    packages.push(Package {
                                        id: id.clone(),
                                        //TODO: get icon from appstream data?
                                        icon: icon_opt.unwrap_or_else(|| {
                                            widget::icon::from_name("package-x-generic").handle()
                                        }),
                                        name: get_translatable(&component.name, &self.locale)
                                            .to_string(),
                                        version: version_opt.unwrap_or("").to_string(),
                                        extra: HashMap::new(),
                                    });
                                }
                            }
                            None => {
                                log::warn!("failed to find component {}", id);
                            }
                        }
                    }
                }
                None => {
                    // Ignore packages with no components
                    log::debug!("no components for package {}", package_name);
                }
            }
        }
        Ok(packages)
    }

    fn appstream(&self, package: &Package) -> Result<Collection, Box<dyn Error>> {
        match self.appstream_cache.collections.get(&package.id) {
            Some(collection) => Ok(collection.clone()),
            None => Err(format!("failed to find component {}", package.id).into()),
        }
    }
}
