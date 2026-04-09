#!/bin/bash
#
# Local CI check script
# Run this script before committing code to ensure it passes all CircleCI checks
#

set -e  # Exit immediately on error

# Color definitions
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print colored messages
print_step() {
    echo -e "${BLUE}==>${NC} $1"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

# Switch to script directory
cd "$(dirname "$0")"

echo "🚀 Starting local CI checks..."
echo ""

# Check 1: Code formatting
print_step "1/6 Checking code format (cargo +nightly fmt)..."

# Check if nightly toolchain is installed
if ! rustup toolchain list | grep -q nightly; then
    print_warning "Nightly toolchain not found, installing..."
    rustup toolchain install nightly
fi

if cargo +nightly fmt -- --check > /dev/null 2>&1; then
    print_success "Code format check passed"
else
    print_error "Code format check failed"
    echo ""
    echo "Please run the following command to fix formatting issues:"
    echo "  cargo +nightly fmt"
    echo "Or use the format script:"
    echo "  ./format.sh"
    exit 1
fi
echo ""

# Check 2: Clippy linting
print_step "2/6 Running Clippy checks (cargo +nightly clippy)..."
if cargo +nightly clippy --all-targets --all-features -- -D warnings 2>&1 | tee /tmp/clippy-output.txt | grep -q "warning\|error"; then
    print_error "Clippy found issues"
    cat /tmp/clippy-output.txt
    echo ""
    echo "Please try to auto-fix with:"
    echo "  cargo +nightly clippy --fix"
    exit 1
else
    print_success "Clippy checks passed"
fi
echo ""

# Check 3: Build project
print_step "3/6 Building project (cargo build)..."
if cargo build --verbose > /dev/null 2>&1; then
    print_success "Debug build succeeded"
else
    print_error "Debug build failed"
    cargo build --verbose
    exit 1
fi

if cargo build --release --verbose > /dev/null 2>&1; then
    print_success "Release build succeeded"
else
    print_error "Release build failed"
    cargo build --release --verbose
    exit 1
fi
echo ""

# Check 4: Run tests
print_step "4/6 Running tests (cargo test)..."
if cargo test --verbose; then
    print_success "All tests passed"
else
    print_error "Tests failed"
    exit 1
fi
echo ""

# Check 5: Code coverage
print_step "5/6 Generating code coverage report..."
if command -v cargo-llvm-cov &> /dev/null; then
    PACKAGE_NAME=$(grep "^name = " Cargo.toml | head -n 1 | sed 's/name = "\(.*\)"/\1/')

    # Generate text format coverage report
    COVERAGE_OUTPUT=$(cargo llvm-cov --package "$PACKAGE_NAME" \
        --ignore-filename-regex "(\.cargo/registry|\.rustup/)" 2>&1)

    # Extract coverage percentage
    COVERAGE_LINE=$(echo "$COVERAGE_OUTPUT" | grep "TOTAL" || echo "")

    if [ -n "$COVERAGE_LINE" ]; then
        print_success "Coverage report generated"
        echo "$COVERAGE_LINE"

        # Check if coverage is below threshold (e.g., 90%)
        LINE_COVERAGE=$(echo "$COVERAGE_LINE" | awk '{print $10}' | sed 's/%//')
        if (( $(echo "$LINE_COVERAGE < 90" | bc -l) )); then
            print_warning "Code coverage ($LINE_COVERAGE%) is below 90%"
        fi
    else
        print_warning "Unable to parse coverage data"
    fi
else
    print_warning "cargo-llvm-cov not installed, skipping coverage check"
    echo "Installation instructions:"
    echo "  cargo install cargo-llvm-cov"
    echo "  rustup component add llvm-tools-preview"
fi
echo ""

# Check 6: Security audit
print_step "6/6 Running security audit (cargo audit)..."
if command -v cargo-audit &> /dev/null; then
    if cargo audit; then
        print_success "Security audit passed, no known vulnerabilities found"
    else
        print_error "Security audit found issues"
        echo ""
        echo "Please review the security issues and consider:"
        echo "  1. Update dependencies: cargo update"
        echo "  2. View details: cargo audit"
        echo "  3. If unable to fix immediately, temporarily ignore in .cargo-audit.toml"
        exit 1
    fi
else
    print_warning "cargo-audit not installed, skipping security audit"
    echo "Installation instructions:"
    echo "  cargo install cargo-audit"
fi
echo ""

# All checks passed
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
print_success "All checks passed! 🎉"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Your code is ready to commit."
echo "After pushing, CircleCI will automatically run the same checks."
echo ""

# Clean up temporary files
rm -f /tmp/clippy-output.txt

