-- cplp-lobby スキーマ定義
-- REQ-LOBBY-001: ロビーサーバーのデータ永続化

-- ユーザー
DEFINE TABLE IF NOT EXISTS users SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name ON users TYPE string;
DEFINE FIELD IF NOT EXISTS email ON users TYPE string;
DEFINE FIELD IF NOT EXISTS avatar_url ON users TYPE option<string>;
DEFINE FIELD IF NOT EXISTS oauth_provider ON users TYPE string;
DEFINE FIELD IF NOT EXISTS oauth_id ON users TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON users TYPE datetime DEFAULT time::now();
DEFINE INDEX IF NOT EXISTS unique_oauth ON users FIELDS oauth_provider, oauth_id UNIQUE;

-- グループ
DEFINE TABLE IF NOT EXISTS groups SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name ON groups TYPE string;
DEFINE FIELD IF NOT EXISTS created_by ON groups TYPE record<users>;
DEFINE FIELD IF NOT EXISTS created_at ON groups TYPE datetime DEFAULT time::now();

-- グループメンバーシップ（リレーション）
DEFINE TABLE IF NOT EXISTS member_of SCHEMAFULL TYPE RELATION FROM users TO groups;
DEFINE FIELD IF NOT EXISTS role ON member_of TYPE string DEFAULT "member";
DEFINE FIELD IF NOT EXISTS joined_at ON member_of TYPE datetime DEFAULT time::now();

-- アクティブセッション
DEFINE TABLE IF NOT EXISTS sessions SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS group_id ON sessions TYPE record<groups>;
DEFINE FIELD IF NOT EXISTS started_by ON sessions TYPE record<users>;
DEFINE FIELD IF NOT EXISTS status ON sessions TYPE string DEFAULT "waiting";
DEFINE FIELD IF NOT EXISTS created_at ON sessions TYPE datetime DEFAULT time::now();

-- セッション参加者（リレーション）
DEFINE TABLE IF NOT EXISTS session_peers SCHEMAFULL TYPE RELATION FROM users TO sessions;
DEFINE FIELD IF NOT EXISTS addr ON session_peers TYPE string;
DEFINE FIELD IF NOT EXISTS joined_at ON session_peers TYPE datetime DEFAULT time::now();
