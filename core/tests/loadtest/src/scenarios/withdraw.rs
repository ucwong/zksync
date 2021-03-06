// Built-in uses
use std::fmt;
// External uses
use async_trait::async_trait;
use num::BigUint;
use serde::{Deserialize, Serialize};
// Workspace uses
use zksync::{types::BlockStatus, utils::closest_packable_token_amount};
// Local uses
use super::{Scenario, ScenarioResources};
use crate::{monitor::Monitor, test_wallet::TestWallet, utils::try_wait_all};

/// Configuration options for the withdraw scenario.
#[derive(Debug, Serialize, Deserialize, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct WithdrawScenarioConfig {
    /// Amount of intermediate wallets to use.
    pub wallets_amount: u64,
    /// Amount of "withdraw and deposit" iterations.
    pub withdraw_rounds: u64,
}

impl Default for WithdrawScenarioConfig {
    fn default() -> Self {
        Self {
            wallets_amount: 5,
            withdraw_rounds: 5,
        }
    }
}

impl From<WithdrawScenarioConfig> for WithdrawScenario {
    fn from(config: WithdrawScenarioConfig) -> Self {
        Self { config }
    }
}

/// Withdraw scenario performs several deposit / withdraw operations.
///
/// The purpose of the withdraw scenario is to ensure that deposits
/// and withdraws are processed correctly when the node is under a
/// load of many transfers.
#[derive(Debug)]
pub struct WithdrawScenario {
    config: WithdrawScenarioConfig,
}

impl fmt::Display for WithdrawScenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("withdraw")
    }
}

#[async_trait]
impl Scenario for WithdrawScenario {
    fn requested_resources(&self, sufficient_fee: &BigUint) -> ScenarioResources {
        let balance_per_wallet = sufficient_fee * (BigUint::from(self.config.withdraw_rounds * 2));

        ScenarioResources {
            wallets_amount: self.config.wallets_amount,
            balance_per_wallet,
        }
    }

    async fn prepare(
        &mut self,
        _monitor: &Monitor,
        _sufficient_fee: &BigUint,
        _wallets: &[TestWallet],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn run(
        &mut self,
        monitor: &Monitor,
        sufficient_fee: &BigUint,
        wallets: &[TestWallet],
    ) -> anyhow::Result<()> {
        for i in 0..self.config.withdraw_rounds {
            log::info!(
                "Withdraw and deposit cycle [{}/{}] started",
                i + 1,
                self.config.withdraw_rounds
            );

            let futures = wallets
                .iter()
                .map(|wallet| Self::withdraw_and_deposit(monitor, sufficient_fee, wallet))
                .collect::<Vec<_>>();
            try_wait_all(futures).await?;

            log::info!(
                "Withdraw and deposit cycle [{}/{}] finished",
                i + 1,
                self.config.withdraw_rounds
            );
        }

        log::info!("Withdraw scenario has been finished");

        Ok(())
    }

    async fn finalize(
        &mut self,
        _monitor: &Monitor,
        _sufficient_fee: &BigUint,
        _wallets: &[TestWallet],
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

impl WithdrawScenario {
    async fn withdraw_and_deposit(
        monitor: &Monitor,
        sufficient_fee: &BigUint,
        wallet: &TestWallet,
    ) -> anyhow::Result<()> {
        let amount = closest_packable_token_amount(
            &(wallet.balance(BlockStatus::Committed).await? - sufficient_fee),
        );

        let (tx, sign) = wallet
            .sign_withdraw(amount.clone(), sufficient_fee.clone())
            .await?;
        monitor
            .wait_for_tx(BlockStatus::Verified, monitor.send_tx(tx, sign).await?)
            .await?;

        await_condition!(
            std::time::Duration::from_millis(1_00),
            wallet.eth_balance().await? >= amount
        );

        let amount = closest_packable_token_amount(&(wallet.eth_balance().await? - sufficient_fee));
        monitor
            .wait_for_priority_op(BlockStatus::Verified, &wallet.deposit(amount).await?)
            .await?;

        Ok(())
    }
}
