#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
UI/UX Pro Max Search - BM25 search engine for UI/UX style guides
Usage: python search.py "<query>" [--domain <domain>] [--stack <stack>] [--max-results 3]
       python search.py "<query>" --design-system [-p "Project Name"]
       python search.py "<query>" --design-system --persist [-p "Project Name"] [--page "dashboard"]

Domains: style, color, chart, landing, product, ux, typography, icons, react, web
Stacks: html-tailwind, react, nextjs, astro, vue, nuxtjs, nuxt-ui, svelte, swiftui,
        react-native, flutter, shadcn, jetpack-compose

Persistence (Master + Overrides pattern):
  --persist    Save design system to design-system/<project-slug>/MASTER.md
  --page       Also create a page-specific override file in
               design-system/<project-slug>/pages/
"""

import argparse
from pathlib import Path
from core import CSV_CONFIG, AVAILABLE_STACKS, MAX_RESULTS, search, search_stack
from design_system import generate_design_system, safe_slug


def positive_int(value: str) -> int:
    """argparse helper that only accepts positive integers."""
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("max-results must be >= 1")
    return parsed


def validate_args(args: argparse.Namespace, parser: argparse.ArgumentParser) -> None:
    """Reject conflicting or no-op argument combinations early."""
    if args.design_system:
        if args.stack or args.domain:
            parser.error("--design-system cannot be combined with --stack or --domain")
        if args.json:
            parser.error("--json is not supported with --design-system")
        if (args.page or args.output_dir) and not args.persist:
            parser.error("--page and --output-dir require --persist with --design-system")
    else:
        design_system_only_args = []
        if args.persist:
            design_system_only_args.append("--persist")
        if args.page:
            design_system_only_args.append("--page")
        if args.output_dir:
            design_system_only_args.append("--output-dir")
        if args.project_name:
            design_system_only_args.append("--project-name")
        if args.format != "ascii":
            design_system_only_args.append("--format")
        if design_system_only_args:
            parser.error(
                f"{', '.join(design_system_only_args)} require --design-system"
            )

    if args.stack and args.domain:
        parser.error("--stack cannot be combined with --domain")


def format_output(result):
    """Format results for Claude consumption (token-optimized)"""
    if "error" in result:
        return f"Error: {result['error']}"

    output = []
    if result.get("stack"):
        output.append(f"## UI Pro Max Stack Guidelines")
        output.append(f"**Stack:** {result['stack']} | **Query:** {result['query']}")
    else:
        output.append(f"## UI Pro Max Search Results")
        output.append(f"**Domain:** {result['domain']} | **Query:** {result['query']}")
    output.append(f"**Source:** {result['file']} | **Found:** {result['count']} results\n")

    for i, row in enumerate(result['results'], 1):
        output.append(f"### Result {i}")
        for key, value in row.items():
            value_str = str(value)
            if len(value_str) > 300:
                value_str = value_str[:300] + "..."
            output.append(f"- **{key}:** {value_str}")
        output.append("")

    return "\n".join(output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="UI Pro Max Search")
    parser.add_argument("query", help="Search query")
    parser.add_argument("--domain", "-d", choices=list(CSV_CONFIG.keys()), help="Search domain")
    parser.add_argument("--stack", "-s", choices=AVAILABLE_STACKS, help="Stack-specific search (html-tailwind, react, nextjs)")
    parser.add_argument("--max-results", "-n", type=positive_int, default=MAX_RESULTS, help="Max results (default: 3)")
    parser.add_argument("--json", action="store_true", help="Output as JSON")
    # Design system generation
    parser.add_argument("--design-system", "-ds", action="store_true", help="Generate complete design system recommendation")
    parser.add_argument("--project-name", "-p", type=str, default=None, help="Project name for design system output")
    parser.add_argument("--format", "-f", choices=["ascii", "markdown"], default="ascii", help="Output format for design system")
    # Persistence (Master + Overrides pattern)
    parser.add_argument(
        "--persist",
        action="store_true",
        help=(
            "Save design system to design-system/<project-slug>/MASTER.md "
            "(creates hierarchical structure)"
        ),
    )
    parser.add_argument(
        "--page",
        type=str,
        default=None,
        help=(
            "Create page-specific override file in "
            "design-system/<project-slug>/pages/"
        ),
    )
    parser.add_argument(
        "--output-dir",
        "-o",
        type=str,
        default=None,
        help="Output directory for persisted files (requires --design-system --persist)",
    )

    args = parser.parse_args()
    validate_args(args, parser)

    # Design system takes priority
    if args.design_system:
        result = generate_design_system(
            args.query, 
            args.project_name, 
            args.format,
            persist=args.persist,
            page=args.page,
            output_dir=args.output_dir
        )
        print(result)
        
        # Print persistence confirmation
        if args.persist:
            project_name = args.project_name or args.query.upper()
            project_slug = safe_slug(project_name, "default")
            page_slug = safe_slug(args.page, "page") if args.page else None
            base_dir = (Path(args.output_dir) if args.output_dir else Path.cwd()).resolve()
            persisted_dir = (base_dir / "design-system" / project_slug).resolve()
            print("\n" + "=" * 60)
            print(f"✅ Design system persisted to {persisted_dir}/")
            print(f"   📄 {persisted_dir / 'MASTER.md'} (Global Source of Truth)")
            if args.page:
                print(f"   📄 {persisted_dir / 'pages' / f'{page_slug}.md'} (Page Overrides)")
            print("")
            print(f"📖 Usage: When building a page, check {persisted_dir / 'pages'}/[page].md first.")
            print(f"   If exists, its rules override MASTER.md. Otherwise, use MASTER.md.")
            print("=" * 60)
    # Stack search
    elif args.stack:
        result = search_stack(args.query, args.stack, args.max_results)
        if args.json:
            import json
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            print(format_output(result))
    # Domain search
    else:
        result = search(args.query, args.domain, args.max_results)
        if args.json:
            import json
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            print(format_output(result))
