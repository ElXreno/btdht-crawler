use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Torrent::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Torrent::InfoHash)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Torrent::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Torrent {
    Table,
    InfoHash,
}
