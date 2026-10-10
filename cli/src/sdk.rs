use anyhow::Result;
use clap::{ArgMatches, Command};

use crate::{
    CliCommand,
    core::command::command,
    sdk::{generate::GenerateCommand, mode::ModeCommand, watch::WatchCommand},
};

pub(crate) mod generate;
pub(crate) mod mode;
mod openapi;
pub(crate) mod watch;

#[derive(Debug)]
pub(crate) struct SdkCommand {
    generate: GenerateCommand,
    mode: ModeCommand,
    watch: WatchCommand,
}

impl SdkCommand {
    pub(crate) fn new() -> Self {
        Self {
            generate: GenerateCommand::new(),
            mode: ModeCommand::new(),
            watch: WatchCommand::new(),
        }
    }
}

impl CliCommand for SdkCommand {
    fn command(&self) -> Command {
        command("sdk", "Manage SDKs")
            .subcommand_required(true)
            .subcommand(self.generate.command())
            .subcommand(self.mode.command())
            .subcommand(self.watch.command())
    }

    fn handler(&self, matches: &ArgMatches) -> Result<()> {
        match matches.subcommand() {
            Some(("generate", sub_matches)) => self.generate.handler(sub_matches),
            Some(("mode", sub_matches)) => self.mode.handler(sub_matches),
            Some(("watch", sub_matches)) => self.watch.handler(sub_matches),
            _ => unreachable!(),
        }
    }
}
