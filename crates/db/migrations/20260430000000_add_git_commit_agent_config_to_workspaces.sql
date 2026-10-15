-- Add git commit agent config to workspaces for custom commit message generation
ALTER TABLE workspaces ADD COLUMN git_commit_agent_config TEXT;
