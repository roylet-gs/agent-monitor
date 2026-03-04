Worktree agent Monitoring CLI Tool:

/Users/roylet/Documents/Projects/gridsight

I want to build a CLI tool that can be used to monitor progress on worktrees within a given repository.
The way this will work is that I call a cli command like "am" (agent monitor) and it will open up a Terminal UI with a list of worktrees
for a given repository.

Startup:

- When starting the tool, it will ask for the repository path. If I have specified this in the past it will ask me to select from a list of repositories.
- When starting the tool for the first time it will ask what will be used to open the worktree. (Cursor, VSCode, Terminal)
- Create the sqlite datebase if it doesn't exist.
- Sync the SQLite database with the list of worktrees in the repository if they are out of sync.

Home Screen:

- The TUI home screen will show a list of all the worktrees in the repository.
- On the left section is the list of worktrees.
- On the right is the details of the selected worktree.
- And the bottom will be a list of key commands that can be used to navigate the TUI like n (new), d (delete), s (settings), q (quit), and whatever else you feel is necessary.

Details of a worktree:

- The details of a worktree will show the following information:
    - Name of the worktree (branch name)
    - Description of the worktree (last commit message)
    - Last updated date of the worktree (last commit date)
    - Git Status of the worktree (ahead/behind, dirty/clean, etc.)
    - The current Claude status of the worktree (idle, error, planning, executing, waiting for user input, etc.)
    - The last response from Claude (if available)

Pressing enter on a worktree:

- Pressing enter on a worktree will open Cursor/VSCode/Terminal window for the selected worktree.

Deleting a worktree:
Deleting a worktree works like a typical git worktree removal, however I want this tool to be smart and consider all the issues that could arise
like uncommitted changes, unpushed changes, missing branches, ect, and preset the user with options to help them resolve the issues.
Deleting a worktree also removes the record from the SQLite database.

Creating a new worktree:
Just like removing a worktree, consider all the edge cases for adding a new worktree, like if the branch already exists, ect.

Technology:

- Workout the best technology stack for this project. There is a great example of a TUI at https://github.com/Gridsight/gridsight/tree/feature/ai-org that you can look at for inspiration.
- To workout the claude status, I want to use Claude hooks that this tool installs into the repository when the user selects a worktree.
- These hooks will be overwritten every time the user selects a worktree so that they are kept up to date if they ever need to change.
- The hooks will send back status information to the tool using the cli tool itself via a command. This command will save the status info to
  a SQLite database stored in ~/.agent-monitor/agent-monitor.db
- The TUI will have a live sync with the database so that the user can see the current status of the worktree in real time.
- When hooks are installed, make sure that the appropriate .gitignore is created so that the hooks don't get committed to the repository.

Statuses:

idle - When nothing is happening and there is no question from the claude
executing - When the permission mode is not plan and claude is actively building.
thinking - Remove this, it should just be part of executing.
planning - When the permission mode is planning show this, even while it is executing.
waiting - When claude is asking the user for a decision. For example, asking a question or waiting for a response to start a plan

Are we able to enable/disable the gh tool PR details in the settings. There may be cases where someone doesn't have the gh tool. Onstartup for the first time Enable it by default, but if the gh tool doesn't exist, disable it and tell the user.
