#debugruntest-compatible

  $ configure modernclient
  $ enable dirsync

  $ newclientrepo repo
  $ mkdir dirA dirB
  $ echo content > dirA/fileA
  $ cat <<EOF > .hgdirsync
  > foo.A = dirA
  > foo.B = dirB
  > EOF
  $ hg commit -Aqm foo

    from pathlib import Path
    import json

    import os
    os.chdir("repo")

    base = sheval("hg whereami")

    ADD = "add"
    MODIFY = "modify"
    REMOVE = "remove"
    REVERT = "revert"
    options = [ADD, MODIFY, REMOVE, REVERT]
    actions = []
    for option1 in options:
        for option2 in options:
            actions.append((option1, option2))

    # Certain combinations are no-ops or invalid, and can be removed.
    # Supporting them would make the loop below more complicated.
    actions.remove((ADD, ADD))
    actions.remove((REMOVE, REMOVE))
    actions.remove((MODIFY, ADD))
    actions.remove((REMOVE, ADD))

    fileA = Path("dirA/fileA")
    original_content = fileA.read_text()

    for (initial, amend) in actions:
        print(f"Initial Action: {initial}, Amend Action: {amend}")

        # Setup the initial pre-amend state of the file.
        if initial == ADD:
            file = Path("dirA/newfile")
            file.write_text("newfile")
        elif initial == MODIFY:
            file = fileA
            file.write_text("modified")
        elif initial == REMOVE:
            file = fileA
            $ hg remove $(py file)
        elif initial == REVERT:
            # A revert only makes sense during the amend phase.
            continue
        else:
            raise ValueError(f"unknown initial action {initial}")

        $ hg commit -Aqm foo
        $ mkdir dirA

        # Modify the file before the amend.
        if amend == ADD:
            file.write_text("new content")
            new_state = "A"
            new_content = file.read_text()
        elif amend == MODIFY:
            file.write_text("modify more")
            new_state = "A" if initial == ADD else "M"
            new_content = file.read_text()
        elif amend == REMOVE:
            $ hg rm $(py file)

            new_state = None if initial == ADD else "R"
            new_content = None
        elif amend == REVERT:
            $ hg revert --rev $(py base) $(py file)

            new_state = None
            new_content = None if initial == ADD else original_content
        else:
            raise ValueError(f"unknown amend action {amend}")

        $ hg amend -q --addremove

        # Verify that the status for each file matches, and the
        # contents/existence matches.
        status = json.loads(sheval("hg st -Tjson --change ."))
        status = {d["path"]: d["status"] for d in status}

        other_file = Path("dirB/" + file.name)

        print(f"file: {file}, other_file: {other_file}, status: {status}")

        # Normalize separator for Windows (since "status" outputs "/").
        file_str = str(file).replace("\\", "/")
        other_file_str = str(other_file).replace("\\", "/")

        assert status.get(file_str) == new_state, f"status: {status}, new_state: {new_state}"
        assert status.get(file_str) == status.get(other_file_str), f"status: {status}"

        if new_content is not None:
            assert file.read_text() == new_content
            assert file.read_text() == other_file.read_text()
        else:
            assert not file.exists()
            assert not other_file.exists()

        # Reset the working copy
        $ hg hide -q .
