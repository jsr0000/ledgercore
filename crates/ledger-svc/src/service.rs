//! The gRPC service implementation. Each handler walks: wire → domain,
//! invoke `LedgerRepo`, domain → wire. Errors at any step funnel
//! through the mapping in [`crate::status`].

use std::sync::Arc;

use app::{Clock, IdGen, LedgerRepo};
use async_trait::async_trait;
use tonic::{Request, Response, Status};

use crate::proto::{
    ledger_server::Ledger, GetAccountHistoryRequest, GetAccountHistoryResponse,
    GetBalanceRequest, GetBalanceResponse, PostTransactionRequest, PostTransactionResponse,
};
use crate::status::{ledger_to_status, wire_to_status};
use crate::wire;

/// gRPC handler. Composes the persistence port with a clock and id
/// generator; no business logic of its own.
pub struct LedgerService {
    repo: Arc<dyn LedgerRepo>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    history_limit_max: u32,
}

impl LedgerService {
    /// Build a service. `history_limit_max` caps `GetAccountHistory`'s
    /// effective limit so a client can't ask for unbounded scans.
    pub fn new(repo: Arc<dyn LedgerRepo>, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self {
            repo,
            clock,
            ids,
            history_limit_max: 1000,
        }
    }
}

#[async_trait]
impl Ledger for LedgerService {
    #[tracing::instrument(
        skip(self, req),
        fields(
            idempotency_key = %req.get_ref().idempotency_key,
            entry_count = req.get_ref().entries.len(),
            rpc = "PostTransaction",
        ),
    )]
    async fn post_transaction(
        &self,
        req: Request<PostTransactionRequest>,
    ) -> Result<Response<PostTransactionResponse>, Status> {
        let req = req.into_inner();
        let tx = wire::to_domain_transaction(req, &*self.clock, &*self.ids)
            .map_err(wire_to_status)?;
        let id = self
            .repo
            .post_transaction(tx)
            .await
            .map_err(ledger_to_status)?;
        Ok(Response::new(PostTransactionResponse {
            transaction_id: id.as_uuid().to_string(),
        }))
    }

    #[tracing::instrument(
        skip(self, req),
        fields(account_id = %req.get_ref().account_id, rpc = "GetBalance"),
    )]
    async fn get_balance(
        &self,
        req: Request<GetBalanceRequest>,
    ) -> Result<Response<GetBalanceResponse>, Status> {
        let req = req.into_inner();
        let account = wire::parse_account_id(&req.account_id).map_err(wire_to_status)?;
        let balance = self.repo.balance(account).await.map_err(ledger_to_status)?;
        Ok(Response::new(GetBalanceResponse {
            balance: Some(wire::from_money(&balance)),
        }))
    }

    #[tracing::instrument(
        skip(self, req),
        fields(
            account_id = %req.get_ref().account_id,
            limit = req.get_ref().limit,
            rpc = "GetAccountHistory",
        ),
    )]
    async fn get_account_history(
        &self,
        req: Request<GetAccountHistoryRequest>,
    ) -> Result<Response<GetAccountHistoryResponse>, Status> {
        let req = req.into_inner();
        let account = wire::parse_account_id(&req.account_id).map_err(wire_to_status)?;
        let limit = req.limit.min(self.history_limit_max);
        let entries = self
            .repo
            .history(account, limit)
            .await
            .map_err(ledger_to_status)?;
        Ok(Response::new(GetAccountHistoryResponse {
            entries: entries.iter().map(wire::from_entry).collect(),
        }))
    }
}
