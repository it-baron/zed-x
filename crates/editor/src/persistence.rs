use anyhow::Result;
use db::{
    query,
    sqlez::{
        bindable::{Bind, Column, StaticColumnCount},
        domain::Domain,
        statement::Statement,
    },
    sqlez_macros::sql,
};
use fs::MTime;
use itertools::Itertools as _;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use workspace::{ItemId, WorkspaceDb, WorkspaceId};

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct SerializedEditor {
    pub(crate) abs_path: Option<PathBuf>,
    pub(crate) contents: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) mtime: Option<MTime>,
}

impl StaticColumnCount for SerializedEditor {
    fn column_count() -> usize {
        6
    }
}

impl Bind for SerializedEditor {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        let start_index = statement.bind(&self.abs_path, start_index)?;
        let start_index = statement.bind(
            &self
                .abs_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            start_index,
        )?;
        let start_index = statement.bind(&self.contents, start_index)?;
        let start_index = statement.bind(&self.language, start_index)?;

        let start_index = match self
            .mtime
            .and_then(|mtime| mtime.to_seconds_and_nanos_for_persistence())
        {
            Some((seconds, nanos)) => {
                let start_index = statement.bind(&(seconds as i64), start_index)?;
                statement.bind(&(nanos as i32), start_index)?
            }
            None => {
                let start_index = statement.bind::<Option<i64>>(&None, start_index)?;
                statement.bind::<Option<i32>>(&None, start_index)?
            }
        };
        Ok(start_index)
    }
}

impl Column for SerializedEditor {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (abs_path, start_index): (Option<PathBuf>, i32) =
            Column::column(statement, start_index)?;
        let (_abs_path, start_index): (Option<PathBuf>, i32) =
            Column::column(statement, start_index)?;
        let (contents, start_index): (Option<String>, i32) =
            Column::column(statement, start_index)?;
        let (language, start_index): (Option<String>, i32) =
            Column::column(statement, start_index)?;
        let (mtime_seconds, start_index): (Option<i64>, i32) =
            Column::column(statement, start_index)?;
        let (mtime_nanos, start_index): (Option<i32>, i32) =
            Column::column(statement, start_index)?;

        let mtime = mtime_seconds
            .zip(mtime_nanos)
            .map(|(seconds, nanos)| MTime::from_seconds_and_nanos(seconds as u64, nanos as u32));

        let editor = Self {
            abs_path,
            contents,
            language,
            mtime,
        };
        Ok((editor, start_index))
    }
}

pub struct EditorDb(db::sqlez::thread_safe_connection::ThreadSafeConnection);

impl Domain for EditorDb {
    const NAME: &str = stringify!(EditorDb);

    // Current schema shape using pseudo-rust syntax:
    // editors(
    //   item_id: usize,
    //   workspace_id: usize,
    //   path: Option<PathBuf>,
    //   scroll_top_row: usize,
    //   scroll_vertical_offset: f32,
    //   scroll_horizontal_offset: f32,
    //   contents: Option<String>,
    //   language: Option<String>,
    //   mtime_seconds: Option<i64>,
    //   mtime_nanos: Option<i32>,
    // )
    //
    // editor_selections(
    //   item_id: usize,
    //   editor_id: usize,
    //   workspace_id: usize,
    //   start: usize,
    //   end: usize,
    // )
    //
    // editor_folds(
    //   item_id: usize,
    //   editor_id: usize,
    //   workspace_id: usize,
    //   start: usize,
    //   end: usize,
    //   start_fingerprint: Option<String>,
    //   end_fingerprint: Option<String>,
    // )

    const MIGRATIONS: &[&str] = &[
        sql! (
            CREATE TABLE editors(
                item_id INTEGER NOT NULL,
                workspace_id INTEGER NOT NULL,
                path BLOB NOT NULL,
                PRIMARY KEY(item_id, workspace_id),
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                ON DELETE CASCADE
                ON UPDATE CASCADE
            ) STRICT;
        ),
        sql! (
            ALTER TABLE editors ADD COLUMN scroll_top_row INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE editors ADD COLUMN scroll_horizontal_offset REAL NOT NULL DEFAULT 0;
            ALTER TABLE editors ADD COLUMN scroll_vertical_offset REAL NOT NULL DEFAULT 0;
        ),
        sql! (
            // Since sqlite3 doesn't support ALTER COLUMN, we create a new
            // table, move the data over, drop the old table, rename new table.
            CREATE TABLE new_editors_tmp (
                item_id INTEGER NOT NULL,
                workspace_id INTEGER NOT NULL,
                path BLOB, // <-- No longer "NOT NULL"
                scroll_top_row INTEGER NOT NULL DEFAULT 0,
                scroll_horizontal_offset REAL NOT NULL DEFAULT 0,
                scroll_vertical_offset REAL NOT NULL DEFAULT 0,
                contents TEXT, // New
                language TEXT, // New
                PRIMARY KEY(item_id, workspace_id),
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                ON DELETE CASCADE
                ON UPDATE CASCADE
            ) STRICT;

            INSERT INTO new_editors_tmp(item_id, workspace_id, path, scroll_top_row, scroll_horizontal_offset, scroll_vertical_offset)
            SELECT item_id, workspace_id, path, scroll_top_row, scroll_horizontal_offset, scroll_vertical_offset
            FROM editors;

            DROP TABLE editors;

            ALTER TABLE new_editors_tmp RENAME TO editors;
        ),
        sql! (
            ALTER TABLE editors ADD COLUMN mtime_seconds INTEGER DEFAULT NULL;
            ALTER TABLE editors ADD COLUMN mtime_nanos INTEGER DEFAULT NULL;
        ),
        sql! (
            CREATE TABLE editor_selections (
                item_id INTEGER NOT NULL,
                editor_id INTEGER NOT NULL,
                workspace_id INTEGER NOT NULL,
                start INTEGER NOT NULL,
                end INTEGER NOT NULL,
                PRIMARY KEY(item_id),
                FOREIGN KEY(editor_id, workspace_id) REFERENCES editors(item_id, workspace_id)
                ON DELETE CASCADE
            ) STRICT;
        ),
        sql! (
            ALTER TABLE editors ADD COLUMN buffer_path TEXT;
            UPDATE editors SET buffer_path = CAST(path AS TEXT);
        ),
        sql! (
            CREATE TABLE editor_folds (
                item_id INTEGER NOT NULL,
                editor_id INTEGER NOT NULL,
                workspace_id INTEGER NOT NULL,
                start INTEGER NOT NULL,
                end INTEGER NOT NULL,
                PRIMARY KEY(item_id),
                FOREIGN KEY(editor_id, workspace_id) REFERENCES editors(item_id, workspace_id)
                ON DELETE CASCADE
            ) STRICT;
        ),
        sql! (
            ALTER TABLE editor_folds ADD COLUMN start_fingerprint TEXT;
            ALTER TABLE editor_folds ADD COLUMN end_fingerprint TEXT;
        ),
        // File-level fold persistence: store folds by file path instead of editor_id.
        // This allows folds to survive tab close and workspace cleanup.
        // Follows the breakpoints pattern in workspace/src/persistence.rs.
        sql! (
            CREATE TABLE file_folds (
                workspace_id INTEGER NOT NULL,
                path TEXT NOT NULL,
                start INTEGER NOT NULL,
                end INTEGER NOT NULL,
                start_fingerprint TEXT,
                end_fingerprint TEXT,
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                PRIMARY KEY(workspace_id, path, start)
            );
        ),
        sql! (
            CREATE TABLE terminal_planning_notes (
                workspace_id INTEGER NOT NULL,
                terminal_item_id INTEGER NOT NULL,
                working_directory_path TEXT NOT NULL,
                note_item_id INTEGER NOT NULL,
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                PRIMARY KEY(workspace_id, terminal_item_id, working_directory_path)
            ) STRICT;
        ),
    ];
}

db::static_connection!(DB, EditorDb, [WorkspaceDb]);

// https://www.sqlite.org/limits.html
// > <..> the maximum value of a host parameter number is SQLITE_MAX_VARIABLE_NUMBER,
// > which defaults to <..> 32766 for SQLite versions after 3.32.0.
const MAX_QUERY_PLACEHOLDERS: usize = 32000;

impl EditorDb {
    query! {
        pub fn get_serialized_editor(item_id: ItemId, workspace_id: WorkspaceId) -> Result<Option<SerializedEditor>> {
            SELECT path, buffer_path, contents, language, mtime_seconds, mtime_nanos FROM editors
            WHERE item_id = ? AND workspace_id = ?
        }
    }

    query! {
        pub async fn save_serialized_editor(item_id: ItemId, workspace_id: WorkspaceId, serialized_editor: SerializedEditor) -> Result<()> {
            INSERT INTO editors
                (item_id, workspace_id, path, buffer_path, contents, language, mtime_seconds, mtime_nanos)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT DO UPDATE SET
                item_id = ?1,
                workspace_id = ?2,
                path = ?3,
                buffer_path = ?4,
                contents = ?5,
                language = ?6,
                mtime_seconds = ?7,
                mtime_nanos = ?8
        }
    }

    // Returns the scroll top row, and offset
    query! {
        pub fn get_scroll_position(item_id: ItemId, workspace_id: WorkspaceId) -> Result<Option<(u32, f64, f64)>> {
            SELECT scroll_top_row, scroll_horizontal_offset, scroll_vertical_offset
            FROM editors
            WHERE item_id = ? AND workspace_id = ?
        }
    }

    query! {
        pub async fn save_scroll_position(
            item_id: ItemId,
            workspace_id: WorkspaceId,
            top_row: u32,
            vertical_offset: f64,
            horizontal_offset: f64
        ) -> Result<()> {
            UPDATE OR IGNORE editors
            SET
                scroll_top_row = ?3,
                scroll_horizontal_offset = ?4,
                scroll_vertical_offset = ?5
            WHERE item_id = ?1 AND workspace_id = ?2
        }
    }

    query! {
        pub fn get_editor_selections(
            editor_id: ItemId,
            workspace_id: WorkspaceId
        ) -> Result<Vec<(usize, usize)>> {
            SELECT start, end
            FROM editor_selections
            WHERE editor_id = ?1 AND workspace_id = ?2
        }
    }

    query! {
        pub fn get_editor_folds(
            editor_id: ItemId,
            workspace_id: WorkspaceId
        ) -> Result<Vec<(usize, usize, Option<String>, Option<String>)>> {
            SELECT start, end, start_fingerprint, end_fingerprint
            FROM editor_folds
            WHERE editor_id = ?1 AND workspace_id = ?2
        }
    }

    query! {
        pub fn get_file_folds(
            workspace_id: WorkspaceId,
            path: &Path
        ) -> Result<Vec<(usize, usize, Option<String>, Option<String>)>> {
            SELECT start, end, start_fingerprint, end_fingerprint
            FROM file_folds
            WHERE workspace_id = ?1 AND path = ?2
            ORDER BY start
        }
    }

    query! {
        pub fn get_terminal_planning_note(
            workspace_id: WorkspaceId,
            terminal_item_id: ItemId,
            working_directory_path: String
        ) -> Result<Option<ItemId>> {
            SELECT note_item_id
            FROM terminal_planning_notes
            WHERE workspace_id = ?1
                AND terminal_item_id = ?2
                AND working_directory_path = ?3
        }
    }

    query! {
        pub fn terminal_planning_note_item_ids(workspace_id: WorkspaceId) -> Result<Vec<ItemId>> {
            SELECT DISTINCT note_item_id
            FROM terminal_planning_notes
            WHERE workspace_id = ?
        }
    }

    pub async fn save_editor_selections(
        &self,
        editor_id: ItemId,
        workspace_id: WorkspaceId,
        selections: Vec<(usize, usize)>,
    ) -> Result<()> {
        log::debug!("Saving selections for editor {editor_id} in workspace {workspace_id:?}");
        let mut first_selection;
        let mut last_selection = 0_usize;
        for (count, placeholders) in std::iter::once("(?1, ?2, ?, ?)")
            .cycle()
            .take(selections.len())
            .chunks(MAX_QUERY_PLACEHOLDERS / 4)
            .into_iter()
            .map(|chunk| {
                let mut count = 0;
                let placeholders = chunk
                    .inspect(|_| {
                        count += 1;
                    })
                    .join(", ");
                (count, placeholders)
            })
            .collect::<Vec<_>>()
        {
            first_selection = last_selection;
            last_selection = last_selection + count;
            let query = format!(
                r#"
DELETE FROM editor_selections WHERE editor_id = ?1 AND workspace_id = ?2;

INSERT OR IGNORE INTO editor_selections (editor_id, workspace_id, start, end)
VALUES {placeholders};
"#
            );

            let selections = selections[first_selection..last_selection].to_vec();
            self.write(move |conn| {
                let mut statement = Statement::prepare(conn, query)?;
                statement.bind(&editor_id, 1)?;
                let mut next_index = statement.bind(&workspace_id, 2)?;
                for (start, end) in selections {
                    next_index = statement.bind(&start, next_index)?;
                    next_index = statement.bind(&end, next_index)?;
                }
                statement.exec()
            })
            .await?;
        }
        Ok(())
    }

    pub async fn save_file_folds(
        &self,
        workspace_id: WorkspaceId,
        path: Arc<Path>,
        folds: Vec<(usize, usize, String, String)>,
    ) -> Result<()> {
        log::debug!("Saving folds for file {path:?} in workspace {workspace_id:?}");
        self.write(move |conn| {
            // Clear existing folds for this file
            conn.exec_bound(sql!(
                DELETE FROM file_folds WHERE workspace_id = ?1 AND path = ?2;
            ))?((workspace_id, path.as_ref()))?;

            // Insert each fold (matches breakpoints pattern)
            for (start, end, start_fp, end_fp) in folds {
                conn.exec_bound(sql!(
                    INSERT INTO file_folds (workspace_id, path, start, end, start_fingerprint, end_fingerprint)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6);
                ))?((workspace_id, path.as_ref(), start, end, start_fp, end_fp))?;
            }
            Ok(())
        })
        .await
    }

    pub async fn delete_file_folds(
        &self,
        workspace_id: WorkspaceId,
        path: Arc<Path>,
    ) -> Result<()> {
        self.write(move |conn| {
            conn.exec_bound(sql!(
                DELETE FROM file_folds WHERE workspace_id = ?1 AND path = ?2;
            ))?((workspace_id, path.as_ref()))
        })
        .await
    }

    pub async fn save_terminal_planning_note(
        &self,
        workspace_id: WorkspaceId,
        terminal_item_id: ItemId,
        working_directory_path: PathBuf,
        note_item_id: ItemId,
    ) -> Result<()> {
        let working_directory_path = working_directory_path.to_string_lossy().into_owned();
        self.write(move |conn| {
            conn.exec_bound(sql!(
                INSERT INTO terminal_planning_notes (
                    workspace_id,
                    terminal_item_id,
                    working_directory_path,
                    note_item_id
                )
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT (workspace_id, terminal_item_id, working_directory_path)
                DO UPDATE SET note_item_id = excluded.note_item_id;
            ))?(
                (
                    workspace_id,
                    terminal_item_id,
                    working_directory_path,
                    note_item_id,
                ),
            )
        })
        .await
    }

    pub async fn delete_terminal_planning_note(
        &self,
        workspace_id: WorkspaceId,
        terminal_item_id: ItemId,
        working_directory_path: PathBuf,
    ) -> Result<()> {
        let working_directory_path = working_directory_path.to_string_lossy().into_owned();
        self.write(move |conn| {
            conn.exec_bound(sql!(
                DELETE FROM terminal_planning_notes
                WHERE workspace_id = ?1
                    AND terminal_item_id = ?2
                    AND working_directory_path = ?3;
            ))?((workspace_id, terminal_item_id, working_directory_path))
        })
        .await
    }

    pub async fn rekey_terminal_planning_note_terminal_item(
        &self,
        workspace_id: WorkspaceId,
        old_terminal_item_id: ItemId,
        new_terminal_item_id: ItemId,
    ) -> Result<()> {
        self.write(move |conn| {
            conn.exec_bound(sql!(
                INSERT INTO terminal_planning_notes (
                    workspace_id,
                    terminal_item_id,
                    working_directory_path,
                    note_item_id
                )
                SELECT
                    workspace_id,
                    ?3,
                    working_directory_path,
                    note_item_id
                FROM terminal_planning_notes
                WHERE workspace_id = ?1 AND terminal_item_id = ?2
                ON CONFLICT (workspace_id, terminal_item_id, working_directory_path)
                DO UPDATE SET note_item_id = excluded.note_item_id;
            ))?((workspace_id, old_terminal_item_id, new_terminal_item_id))?;

            conn.exec_bound(sql!(
                DELETE FROM terminal_planning_notes
                WHERE workspace_id = ?1 AND terminal_item_id = ?2;
            ))?((workspace_id, old_terminal_item_id))
        })
        .await
    }

    query! {
        pub async fn rekey_terminal_planning_note_item(
            workspace_id: WorkspaceId,
            old_note_item_id: ItemId,
            new_note_item_id: ItemId
        ) -> Result<()> {
            UPDATE terminal_planning_notes
            SET note_item_id = ?3
            WHERE workspace_id = ?1 AND note_item_id = ?2
        }
    }

    pub async fn delete_unloaded_terminal_planning_notes(
        &self,
        workspace_id: WorkspaceId,
        alive_terminal_ids: Vec<ItemId>,
    ) -> Result<()> {
        let placeholders = alive_terminal_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");

        let query = if alive_terminal_ids.is_empty() {
            "DELETE FROM terminal_planning_notes WHERE workspace_id = ?1".to_string()
        } else {
            format!(
                "DELETE FROM terminal_planning_notes WHERE workspace_id = ?1 AND terminal_item_id NOT IN ({placeholders})"
            )
        };

        self.write(move |conn| {
            let mut statement = Statement::prepare(conn, query)?;
            let mut next_index = statement.bind(&workspace_id, 1)?;
            for item_id in alive_terminal_ids {
                next_index = statement.bind(&item_id, next_index)?;
            }
            statement.exec()
        })
        .await
    }
}

pub fn get_terminal_planning_note(
    workspace_id: WorkspaceId,
    terminal_item_id: ItemId,
    working_directory_path: &Path,
) -> Result<Option<ItemId>> {
    DB.get_terminal_planning_note(
        workspace_id,
        terminal_item_id,
        working_directory_path.to_string_lossy().into_owned(),
    )
}

pub async fn save_terminal_planning_note(
    workspace_id: WorkspaceId,
    terminal_item_id: ItemId,
    working_directory_path: PathBuf,
    note_item_id: ItemId,
) -> Result<()> {
    DB.save_terminal_planning_note(
        workspace_id,
        terminal_item_id,
        working_directory_path,
        note_item_id,
    )
    .await
}

pub async fn delete_terminal_planning_note(
    workspace_id: WorkspaceId,
    terminal_item_id: ItemId,
    working_directory_path: PathBuf,
) -> Result<()> {
    DB.delete_terminal_planning_note(workspace_id, terminal_item_id, working_directory_path)
        .await
}

pub async fn rekey_terminal_planning_note_terminal_item(
    workspace_id: WorkspaceId,
    old_terminal_item_id: ItemId,
    new_terminal_item_id: ItemId,
) -> Result<()> {
    DB.rekey_terminal_planning_note_terminal_item(
        workspace_id,
        old_terminal_item_id,
        new_terminal_item_id,
    )
    .await
}

pub async fn rekey_terminal_planning_note_item(
    workspace_id: WorkspaceId,
    old_note_item_id: ItemId,
    new_note_item_id: ItemId,
) -> Result<()> {
    DB.rekey_terminal_planning_note_item(workspace_id, old_note_item_id, new_note_item_id)
        .await
}

pub async fn delete_unloaded_terminal_planning_notes(
    workspace_id: WorkspaceId,
    alive_terminal_ids: Vec<ItemId>,
) -> Result<()> {
    DB.delete_unloaded_terminal_planning_notes(workspace_id, alive_terminal_ids)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    async fn test_save_and_get_serialized_editor() {
        let workspace_id = workspace::WORKSPACE_DB.next_id().await.unwrap();

        let serialized_editor = SerializedEditor {
            abs_path: Some(PathBuf::from("testing.txt")),
            contents: None,
            language: None,
            mtime: None,
        };

        DB.save_serialized_editor(1234, workspace_id, serialized_editor.clone())
            .await
            .unwrap();

        let have = DB
            .get_serialized_editor(1234, workspace_id)
            .unwrap()
            .unwrap();
        assert_eq!(have, serialized_editor);

        // Now update contents and language
        let serialized_editor = SerializedEditor {
            abs_path: Some(PathBuf::from("testing.txt")),
            contents: Some("Test".to_owned()),
            language: Some("Go".to_owned()),
            mtime: None,
        };

        DB.save_serialized_editor(1234, workspace_id, serialized_editor.clone())
            .await
            .unwrap();

        let have = DB
            .get_serialized_editor(1234, workspace_id)
            .unwrap()
            .unwrap();
        assert_eq!(have, serialized_editor);

        // Now set all the fields to NULL
        let serialized_editor = SerializedEditor {
            abs_path: None,
            contents: None,
            language: None,
            mtime: None,
        };

        DB.save_serialized_editor(1234, workspace_id, serialized_editor.clone())
            .await
            .unwrap();

        let have = DB
            .get_serialized_editor(1234, workspace_id)
            .unwrap()
            .unwrap();
        assert_eq!(have, serialized_editor);

        // Storing and retrieving mtime
        let serialized_editor = SerializedEditor {
            abs_path: None,
            contents: None,
            language: None,
            mtime: Some(MTime::from_seconds_and_nanos(100, 42)),
        };

        DB.save_serialized_editor(1234, workspace_id, serialized_editor.clone())
            .await
            .unwrap();

        let have = DB
            .get_serialized_editor(1234, workspace_id)
            .unwrap()
            .unwrap();
        assert_eq!(have, serialized_editor);
    }

    // NOTE: The fingerprint search logic (finding content at new offsets when file
    // is modified externally) is in editor.rs:restore_from_db and requires a full
    // Editor context to test. Manual testing procedure:
    // 1. Open a file, fold some sections, close Zed
    // 2. Add text at the START of the file externally (shifts all offsets)
    // 3. Reopen Zed - folds should be restored at their NEW correct positions
    // The search uses contains_str_at() to find fingerprints in the buffer.

    #[gpui::test]
    async fn test_save_and_get_file_folds() {
        let workspace_id = workspace::WORKSPACE_DB.next_id().await.unwrap();

        // file_folds table uses path as key (no FK to editors table)
        let file_path: Arc<Path> = Arc::from(Path::new("/tmp/test_file_folds.rs"));

        // Save folds with fingerprints
        let folds = vec![
            (
                100,
                200,
                "fn main() {".to_string(),
                "} // end main".to_string(),
            ),
            (
                300,
                400,
                "struct Foo {".to_string(),
                "} // end Foo".to_string(),
            ),
        ];
        DB.save_file_folds(workspace_id, file_path.clone(), folds.clone())
            .await
            .unwrap();

        // Retrieve and verify fingerprints are preserved
        let retrieved = DB.get_file_folds(workspace_id, &file_path).unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(
            retrieved[0],
            (
                100,
                200,
                Some("fn main() {".to_string()),
                Some("} // end main".to_string())
            )
        );
        assert_eq!(
            retrieved[1],
            (
                300,
                400,
                Some("struct Foo {".to_string()),
                Some("} // end Foo".to_string())
            )
        );

        // Test overwrite: saving new folds replaces old ones
        let new_folds = vec![(
            500,
            600,
            "impl Bar {".to_string(),
            "} // end impl".to_string(),
        )];
        DB.save_file_folds(workspace_id, file_path.clone(), new_folds)
            .await
            .unwrap();

        let retrieved = DB.get_file_folds(workspace_id, &file_path).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(
            retrieved[0],
            (
                500,
                600,
                Some("impl Bar {".to_string()),
                Some("} // end impl".to_string())
            )
        );

        // Test delete
        DB.delete_file_folds(workspace_id, file_path.clone())
            .await
            .unwrap();
        let retrieved = DB.get_file_folds(workspace_id, &file_path).unwrap();
        assert!(retrieved.is_empty());

        // Test multiple files don't interfere
        let file_path_a: Arc<Path> = Arc::from(Path::new("/tmp/file_a.rs"));
        let file_path_b: Arc<Path> = Arc::from(Path::new("/tmp/file_b.rs"));
        let folds_a = vec![(10, 20, "a_start".to_string(), "a_end".to_string())];
        let folds_b = vec![(30, 40, "b_start".to_string(), "b_end".to_string())];

        DB.save_file_folds(workspace_id, file_path_a.clone(), folds_a)
            .await
            .unwrap();
        DB.save_file_folds(workspace_id, file_path_b.clone(), folds_b)
            .await
            .unwrap();

        let retrieved_a = DB.get_file_folds(workspace_id, &file_path_a).unwrap();
        let retrieved_b = DB.get_file_folds(workspace_id, &file_path_b).unwrap();

        assert_eq!(retrieved_a.len(), 1);
        assert_eq!(retrieved_b.len(), 1);
        assert_eq!(retrieved_a[0].0, 10); // file_a's fold
        assert_eq!(retrieved_b[0].0, 30); // file_b's fold
    }

    #[gpui::test]
    async fn test_terminal_planning_notes_round_trip_and_rekey() {
        let workspace_id = workspace::WORKSPACE_DB.next_id().await.unwrap();
        let cwd = PathBuf::from("/tmp/project-a");

        DB.save_terminal_planning_note(workspace_id, 11, cwd.clone(), 101)
            .await
            .unwrap();

        assert_eq!(
            DB.get_terminal_planning_note(workspace_id, 11, cwd.to_string_lossy().into_owned())
                .unwrap(),
            Some(101)
        );
        assert_eq!(
            DB.terminal_planning_note_item_ids(workspace_id).unwrap(),
            vec![101]
        );

        DB.rekey_terminal_planning_note_terminal_item(workspace_id, 11, 22)
            .await
            .unwrap();
        assert_eq!(
            DB.get_terminal_planning_note(workspace_id, 22, cwd.to_string_lossy().into_owned())
                .unwrap(),
            Some(101)
        );
        assert_eq!(
            DB.get_terminal_planning_note(workspace_id, 11, cwd.to_string_lossy().into_owned())
                .unwrap(),
            None
        );

        DB.rekey_terminal_planning_note_item(workspace_id, 101, 202)
            .await
            .unwrap();
        assert_eq!(
            DB.get_terminal_planning_note(workspace_id, 22, cwd.to_string_lossy().into_owned())
                .unwrap(),
            Some(202)
        );
        assert_eq!(
            DB.terminal_planning_note_item_ids(workspace_id).unwrap(),
            vec![202]
        );

        DB.delete_terminal_planning_note(workspace_id, 22, cwd.clone())
            .await
            .unwrap();
        assert_eq!(
            DB.get_terminal_planning_note(workspace_id, 22, cwd.to_string_lossy().into_owned())
                .unwrap(),
            None
        );
        assert!(DB.terminal_planning_note_item_ids(workspace_id)
            .unwrap()
            .is_empty());
    }

    #[gpui::test]
    async fn test_delete_unloaded_terminal_planning_notes() {
        let workspace_id = workspace::WORKSPACE_DB.next_id().await.unwrap();

        DB.save_terminal_planning_note(workspace_id, 1, PathBuf::from("/tmp/a"), 10)
            .await
            .unwrap();
        DB.save_terminal_planning_note(workspace_id, 2, PathBuf::from("/tmp/b"), 20)
            .await
            .unwrap();

        DB.delete_unloaded_terminal_planning_notes(workspace_id, vec![2])
            .await
            .unwrap();

        assert_eq!(
            DB.terminal_planning_note_item_ids(workspace_id).unwrap(),
            vec![20]
        );

        DB.delete_unloaded_terminal_planning_notes(workspace_id, Vec::new())
            .await
            .unwrap();

        assert!(DB.terminal_planning_note_item_ids(workspace_id).unwrap().is_empty());
    }
}
