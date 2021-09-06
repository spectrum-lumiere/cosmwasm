use crate::query::CustomQuery;
use crate::results::Empty;
use crate::traits::{Api, Querier, Storage};
use crate::QuerierWrapper;

/// Holds all external dependencies of the contract.
/// Designed to allow easy dependency injection at runtime.
/// This cannot be copied or cloned since it would behave differently
/// for mock storages and a bridge storage in the VM.
pub struct OwnedDeps<S: Storage, A: Api, Q: Querier> {
    pub storage: S,
    pub api: A,
    pub querier: Q,
}

pub struct DepsMut<'a> {
    pub storage: &'a mut dyn Storage,
    pub api: &'a dyn Api,
    /// Do not use this raw querier directly.
    /// Use [`DepsMut::querier()`] or [`DepsMut::custom_querier()`] instead.
    querier: &'a dyn Querier,
}

#[derive(Copy, Clone)]
pub struct Deps<'a> {
    pub storage: &'a dyn Storage,
    pub api: &'a dyn Api,
    /// Do not use this raw querier directly.
    /// Use [`Deps::querier()`] or [`Deps::custom_querier()`] instead.
    querier: &'a dyn Querier,
}

impl<S: Storage, A: Api, Q: Querier> OwnedDeps<S, A, Q> {
    pub fn as_ref(&'_ self) -> Deps<'_> {
        Deps {
            storage: &self.storage,
            api: &self.api,
            querier: &self.querier,
        }
    }

    pub fn as_mut(&'_ mut self) -> DepsMut<'_> {
        DepsMut {
            storage: &mut self.storage,
            api: &self.api,
            querier: &self.querier,
        }
    }
}

impl<'a> DepsMut<'a> {
    pub fn as_ref(&'_ self) -> Deps<'_> {
        Deps {
            storage: self.storage,
            api: self.api,
            querier: self.querier,
        }
    }

    pub fn branch(&'_ mut self) -> DepsMut<'_> {
        DepsMut {
            storage: self.storage,
            api: self.api,
            querier: self.querier,
        }
    }

    /// Creates a `QuerierWrapper` that allows you to use the querier.
    /// This version does not support custom query types.
    /// See also [`custom_querier()`] for a more advanced version.
    pub fn querier(&'_ self) -> QuerierWrapper<'_, Empty> {
        self.custom_querier::<Empty>()
    }

    /// Creates a `QuerierWrapper` that allows you to use the querier.
    /// This version supports custom query types.
    /// See also [`querier()`] for a simpler version.
    pub fn custom_querier<C: CustomQuery>(&'_ self) -> QuerierWrapper<'_, C> {
        QuerierWrapper::<C>::new(self.querier)
    }
}

impl<'a> Deps<'a> {
    /// Creates a `QuerierWrapper` that allows you to use the querier.
    /// This version does not support custom query types.
    /// See also [`custom_querier()`] for a more advanced version.
    pub fn querier(&'_ self) -> QuerierWrapper<'_, Empty> {
        self.custom_querier::<Empty>()
    }

    /// Creates a `QuerierWrapper` that allows you to use the querier.
    /// This version supports custom query types.
    /// See also [`querier()`] for a simpler version.
    pub fn custom_querier<C: CustomQuery>(&'_ self) -> QuerierWrapper<'_, C> {
        QuerierWrapper::<C>::new(self.querier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::mock_dependencies;

    // ensure we can call these many times, eg. as sub-calls
    fn execute(mut deps: DepsMut) {
        execute2(deps.branch());
        query(deps.as_ref());
        execute2(deps.branch());
    }
    fn execute2(_deps: DepsMut) {}

    fn query(deps: Deps) {
        query2(deps);
        query2(deps);
    }
    fn query2(_deps: Deps) {}

    #[test]
    fn ensure_easy_reuse() {
        let mut deps = mock_dependencies(&[]);
        execute(deps.as_mut());
        query(deps.as_ref())
    }
}
