/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

pub struct SqlBonsaiTagMapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlBonsaiTagMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlBonsaiTagMappingBuilder {
    const LABEL: &'static str = "bonsai_tag_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-tag-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiTagMappingBuilder {}

impl SqlBonsaiTagMappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlBonsaiTagMapping {
        SqlBonsaiTagMapping {
            connections: self.connections,
            repo_id,
        }
    }
}
