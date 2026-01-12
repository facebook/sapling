# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Problem categories"""
# rage_categories.py

# First define all categories except "other"
_BASE_CATEGORIES = {
    "cmd_perf": {
        "name": "cmd_perf",
        "display_name": "Command Performance",
        "description": "Hanging, slow, or timing out operations",
        "sections": [
            "sigtrace",
            "blackbox",
            "sl_st",
            "sl_sl",
            "debugprocesstree",
            "debugnetwork",
            "debugnetworkdoctor",
            "commitcloud_state",
            "cloud_status",
            "eden_logs",
            "watchman_logs",
            "eden_memory",
            "strobelite_traces",
            "quickstack",
            "edenfs_counters",
            "disk_space",
            "system_load",
            "eden_top",
            "eden_recent_events",
            "sl_doctor",
            "eden_doctor",
        ],
        "timeout": 180,
        "collector": "perf",
    },
    "resource_exhaustion": {
        "name": "resource_exhaustion",
        "display_name": "Resource Exhaustion",
        "description": "Memory, disk, or CPU resource problems",
        "sections": [
            "sigtrace",
            "blackbox",
            "sl_st",
            "sl_sl",
            "debugprocesstree",
            "debugnetwork",
            "debugnetworkdoctor",
            "commitcloud_state",
            "cloud_status",
            "eden_logs",
            "watchman_logs",
            "eden_memory",
            "strobelite_traces",
            "quickstack",
            "edenfs_counters",
            "disk_space",
            "system_load",
            "eden_top",
            "eden_recent_events",
            "sl_doctor",
            "eden_doctor",
        ],
        "timeout": 180,
        "collector": "perf",
    },
    "smartlog_cloud": {
        "name": "smartlog_cloud",
        "display_name": "Smartlog & Commit Cloud Sync",
        "description": "Missing commits, too many commits and cloud synchronization problems",
        "sections": [
            "sl_sl",
            "debugmetalog",
            "cloud_status",
            "debugmutation",
            "commitcloud_state",
        ],
        "timeout": 60,
        "collector": "sync",
    },
    "checkout_rebase": {
        "name": "checkout_rebase",
        "display_name": "Checkout / Rebase related",
        "description": "Stack related issues, merge conflicts, divergence problems",
        "sections": [
            "debugmetalog",
            "debugmutation",
            "sl_sl",
            "sl_st",
            "blackbox",
            "eden_logs",
            "watchman_logs",
            "sigtrace",
        ],
        "timeout": 60,
        "collector": "system",
    },
    "eden_doctor": {
        "name": "eden_doctor",
        "display_name": "Sl/Eden doctor errors with no actionable error message",
        "description": "UnexpectedMountProblem, BackingRepoCorruption, socket timeouts, etc.",
        "sections": [
            "eden_logs",
            "eden_memory",
            "edenfs_counters",
            "eden_recent_events",
            "disk_space",
        ],
        "timeout": 60,
        "collector": "system",
    },
}


def _get_all_unique_sections():
    """Get all unique sections from all base categories"""
    all_sections = set()
    for category_info in _BASE_CATEGORIES.values():
        all_sections.update(category_info.get("sections", []))
    return sorted(list(all_sections))


CATEGORIES = _BASE_CATEGORIES.copy()
CATEGORIES["other"] = {
    "name": "other",
    "display_name": "Other (collect everything)",
    "sections": _get_all_unique_sections(),
    "timeout": 300,  # Longer timeout since it collects more data
    "collector": "all",
}


def get_available_categories():
    """Get all available problem categories"""
    return CATEGORIES.copy()


def get_category_info(category_name):
    """Get information for a specific category"""
    return CATEGORIES.get(category_name)


# Helper functions to eliminate code duplication
def format_sections_display(sections, max_sections=5):
    """Format sections for display with truncation if needed"""
    if len(sections) <= max_sections:
        return ", ".join(sections)
    else:
        return (
            ", ".join(sections[:max_sections])
            + f" (+{len(sections) - max_sections} more)"
        )


def display_category_for_list(ui, name, info):
    """Display category info for --list-categories command"""
    # Handle missing description gracefully
    if "description" in info:
        ui.write(f"  {name:<20} - {info['description']}\n")
    else:
        ui.write(f"  {name:<20}\n")

    # Show sections (truncate if too many)
    sections = info["sections"]
    sections_str = format_sections_display(sections)
    ui.write(f"{'':23} Sections: {sections_str}\n")

    # timeout info
    timeout = info.get("timeout", 60)
    ui.write(f"{'':23} Timeout: {timeout}s\n\n")


def display_category_summary(ui, category_info):
    """Display category summary for --category command"""
    ui.write(f" Using category: {category_info['display_name']}\n")
    # Handle missing description gracefully
    if "description" in category_info:
        ui.write(f"   Description: {category_info['description']}\n")
    ui.write(f"   Sections: {len(category_info['sections'])} diagnostic sections\n")
    ui.write(f"   Estimated time: {category_info.get('timeout', 60)} seconds\n\n")


def interactive_category_selection(ui):
    """Simple interactive category selection"""
    categories = get_available_categories()

    # Display header
    ui.write("\n" + "=" * 70 + "\n")
    ui.write(" Enhanced Rage - Problem Category Selection\n")
    ui.write("=" * 70 + "\n\n")

    ui.write("Please select the type of problem you're experiencing:\n\n")

    # Display categories
    category_items = list(categories.items())
    for i, (key, category) in enumerate(category_items, 1):
        ui.write(f"{i}. {category['display_name']}\n")
        description = category.get("description", "").strip()
        if description:
            ui.write(f"   {category['description']}\n")

        # Show timeout information
        timeout = category.get("timeout", 60)
        ui.write(f"   Estimated time: {timeout} seconds\n\n")

    ui.write(f"{len(category_items) + 1}. Cancel (run standard rage instead)\n\n")

    # Get user selection
    max_choice = len(category_items) + 1

    while True:
        try:
            choice_str = ui.prompt(
                f"Select category (1-{max_choice}): ",
                default=str(max_choice),
            )

            choice = int(choice_str.strip())

            if 1 <= choice <= len(category_items):
                selected_category = category_items[choice - 1][0]
                selected_info = category_items[choice - 1][1]

                # Confirm selection - handle missing description gracefully
                ui.write(f"\n Selected: {selected_info['display_name']}\n")
                if "description" in selected_info:
                    ui.write(f"  Description: {selected_info['description']}\n")
                ui.write(
                    f"  Estimated time: {selected_info.get('timeout', 60)} seconds\n\n"
                )

                return selected_category

            elif choice == max_choice:
                ui.write("\n Selection cancelled. Running standard rage...\n")
                return None
            else:
                ui.write(
                    f"\nInvalid choice. Please enter a number between 1 and {max_choice}.\n\n"
                )
                continue

        except ValueError:
            ui.write(
                f"\nInvalid input. Please enter a number between 1 and {max_choice}.\n\n"
            )
            continue
        except (EOFError, KeyboardInterrupt):
            ui.write("\n\nSelection cancelled.\n")
            raise KeyboardInterrupt()
