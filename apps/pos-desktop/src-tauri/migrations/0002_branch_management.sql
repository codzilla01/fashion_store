-- Additive security migration: branch administration is owner-only by default.
INSERT OR IGNORE INTO permissions(code, description)
VALUES ('branches.manage', 'Create branches and assign staff to branches');

INSERT OR IGNORE INTO role_permissions(role_code, permission_code)
VALUES ('owner', 'branches.manage');
