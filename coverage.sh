#!/bin/bash
#
# Code coverage testing script
# Uses cargo-llvm-cov to generate code coverage reports
#

set -e

echo "🔍 Starting code coverage testing..."

# Switch to project directory
cd "$(dirname "$0")"

# Detect package name from Cargo.toml
if [ -f "Cargo.toml" ]; then
    PACKAGE_NAME=$(grep "^name = " Cargo.toml | head -n 1 | sed 's/name = "\(.*\)"/\1/')
    echo "📦 Detected package: $PACKAGE_NAME"
else
    echo "❌ Error: Cargo.toml not found in current directory"
    exit 1
fi

# Get current directory absolute path to filter coverage
CURRENT_CRATE_DIR=$(pwd)
echo "📁 Coverage will only include files in: $CURRENT_CRATE_DIR"

# Build regex pattern to exclude third-party code and other workspace members
CURRENT_CRATE_NAME=$(basename "$CURRENT_CRATE_DIR")
WORKSPACE_ROOT=$(cd "$(dirname "$0")/.." && pwd)

# Create list of other workspace crates to exclude
OTHER_CRATES=""
for crate_dir in "$WORKSPACE_ROOT"/rust-*/; do
    crate_name=$(basename "$crate_dir")
    if [ "$crate_name" != "$CURRENT_CRATE_NAME" ]; then
        if [ -z "$OTHER_CRATES" ]; then
            OTHER_CRATES="$crate_name"
        else
            OTHER_CRATES="$OTHER_CRATES|$crate_name"
        fi
    fi
done

# Exclude: cargo registry, rustup, and other workspace crates.
# Do not use "/(crate)/" alone: paths like ".../rust-common/rust-http/src/..." contain
# the workspace folder name "rust-common", which matches sibling "rust-common" and zeroes coverage.
if command -v python3 >/dev/null 2>&1; then
    ESCAPED_WORKSPACE=$(python3 -c 'import re,sys; print(re.escape(sys.argv[1]))' "$WORKSPACE_ROOT")
else
    ESCAPED_WORKSPACE=$(printf '%s' "$WORKSPACE_ROOT" | sed 's/[.[\\*^$()+?{|]/\\&/g')
fi
if [ -n "$OTHER_CRATES" ]; then
    EXCLUDE_PATTERN="(\.cargo/registry|\.rustup/|${ESCAPED_WORKSPACE}/(${OTHER_CRATES})/)"
else
    EXCLUDE_PATTERN="(\.cargo/registry|\.rustup/)"
fi
echo "🚫 Excluding: .cargo/registry, .rustup, and sibling rust-* crates under workspace root"

# Parse arguments, check if cleanup is needed
CLEAN_FLAG=""
FORMAT_ARG=""

for arg in "$@"; do
    case "$arg" in
        --clean)
            CLEAN_FLAG="yes"
            ;;
        *)
            FORMAT_ARG="$arg"
            ;;
    esac
done

# Default format is html
FORMAT_ARG="${FORMAT_ARG:-html}"

# If --clean option is specified, clean old data
if [ "$CLEAN_FLAG" = "yes" ]; then
    echo "🧹 Cleaning old coverage data..."
    cargo llvm-cov clean
else
    echo "ℹ️  Using cached build (use --clean option if you need to clean cache)"
fi

# Run tests and generate coverage reports
case "$FORMAT_ARG" in
    html)
        echo "📊 Generating HTML format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --html --open \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        echo "✅ HTML report generated and opened in browser"
        echo "   Report location: target/llvm-cov/html/index.html"
        ;;

    text)
        echo "📊 Generating text format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        ;;

    lcov)
        echo "📊 Generating LCOV format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --lcov --output-path target/llvm-cov/lcov.info \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        echo "✅ LCOV report generated"
        echo "   Report location: target/llvm-cov/lcov.info"
        ;;

    json)
        echo "📊 Generating JSON format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --json --output-path target/llvm-cov/coverage.json \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        echo "✅ JSON report generated"
        echo "   Report location: target/llvm-cov/coverage.json"
        ;;

    cobertura)
        echo "📊 Generating Cobertura XML format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --cobertura --output-path target/llvm-cov/cobertura.xml \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        echo "✅ Cobertura report generated"
        echo "   Report location: target/llvm-cov/cobertura.xml"
        ;;

    all)
        echo "📊 Generating all format coverage reports..."

        # HTML
        echo "  - Generating HTML report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --html \
            --ignore-filename-regex "$EXCLUDE_PATTERN"

        # LCOV
        echo "  - Generating LCOV report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --lcov --output-path target/llvm-cov/lcov.info \
            --ignore-filename-regex "$EXCLUDE_PATTERN"

        # JSON
        echo "  - Generating JSON report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --json --output-path target/llvm-cov/coverage.json \
            --ignore-filename-regex "$EXCLUDE_PATTERN"

        # Cobertura
        echo "  - Generating Cobertura XML report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --cobertura --output-path target/llvm-cov/cobertura.xml \
            --ignore-filename-regex "$EXCLUDE_PATTERN"

        echo "✅ All format reports generated"
        echo "   HTML:      target/llvm-cov/html/index.html"
        echo "   LCOV:      target/llvm-cov/lcov.info"
        echo "   JSON:      target/llvm-cov/coverage.json"
        echo "   Cobertura: target/llvm-cov/cobertura.xml"
        ;;

    help|--help|-h)
        echo "Usage: ./coverage.sh [format] [options]"
        echo ""
        echo "Format options:"
        echo "  html       Generate HTML report and open in browser (default)"
        echo "  text       Output text format report to terminal"
        echo "  lcov       Generate LCOV format report"
        echo "  json       Generate JSON format report"
        echo "  cobertura  Generate Cobertura XML format report"
        echo "  all        Generate all format reports"
        echo "  help       Show this help information"
        echo ""
        echo "Options:"
        echo "  --clean    Clean old coverage data and build cache before running"
        echo "             By default, cached builds are used to speed up compilation"
        echo ""
        echo "Performance tips:"
        echo "  • First run will be slower (needs to compile all dependencies)"
        echo "  • Subsequent runs will be much faster (using cache)"
        echo "  • Only use --clean when dependencies are updated or major code changes"
        echo ""
        echo "Examples:"
        echo "  ./coverage.sh              # Generate HTML report (using cache)"
        echo "  ./coverage.sh text         # Output text report (using cache)"
        echo "  ./coverage.sh --clean      # Clean then generate HTML report"
        echo "  ./coverage.sh html --clean # Clean then generate HTML report"
        echo "  ./coverage.sh all --clean  # Clean then generate all formats"
        exit 0
        ;;

    *)
        echo "❌ Error: Unknown format '$1'"
        echo "Run './coverage.sh help' to see available options"
        exit 1
        ;;
esac

echo "✅ Code coverage testing completed!"

