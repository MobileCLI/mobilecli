# macOS Hosting Options for MobileCLI Testing & CI/CD

## Research Summary (2026-01-31)

### Cloud Providers Offering macOS

#### 1. **Scaleway**
- **Pricing**: ~0.11 EUR/hour (~€90-109/month)
- **Hardware**: Mac mini M1/M2
- **Minimum Billing**: 24 hours
- **Pros**: European data centers, hourly billing, good integration with CI/CD
- **Cons**: 24-hour minimum commitment

#### 2. **AWS EC2 Mac**
- **Pricing**: More expensive than Scaleway/MacStadium
- **Hardware**: Mac mini (Intel & M1/M2)
- **Minimum Billing**: 24 hours
- **Pros**: AWS ecosystem integration, reliable infrastructure
- **Cons**: High cost, 24-hour minimum

#### 3. **MacStadium**
- **Pricing**: $109/month for M1 Mac mini (8GB/256GB)
- **Hardware**: Various Mac mini configurations
- **Pros**: Mac-focused provider, multiple options, good support
- **Cons**: Monthly commitment, slightly more than Scaleway

#### 4. **MacinCloud**
- **Pricing**: Various tiers available
- **Hardware**: Range of Mac configurations
- **Pros**: Established provider, good for development
- **Cons**: Can be pricey for continuous use

### Hetzner Status
- **No longer offers Mac hosting** (discontinued ~2023)
- Previously offered Mac mini servers
- Community suggests OakHost (uses Hetzner data centers) as alternative

### Self-Hosted Options

#### GitHub Actions Self-Hosted Runner
- Can run on your own Mac mini
- Best for organizations with existing hardware
- Complete control over environment
- No recurring cloud costs

#### Virtualization Solutions
- **Anka** (Veertu): macOS virtualization for CI/CD
- **VirtualBuddy**: Local macOS VM management
- Note: Apple licensing restricts macOS VMs to Apple hardware only

### Recommendations for MobileCLI

1. **For Development/Testing**: 
   - Scaleway (hourly billing, reasonable rates)
   - Good for intermittent testing needs

2. **For Continuous CI/CD**:
   - MacStadium (reliable, Mac-focused)
   - Self-hosted Mac mini with GitHub Actions

3. **Budget Option**:
   - Scaleway on-demand (pay only when needed)
   - 24-hour minimum still cheaper than continuous monthly

### Apple License Compliance
- macOS can only be virtualized on Apple hardware
- All listed cloud providers use genuine Mac hardware
- No legal way to run macOS on non-Apple servers

### Integration with CI/CD
All providers support:
- SSH access
- GitHub Actions runners
- GitLab CI/CD
- Jenkins
- Custom automation scripts

### Next Steps
1. Set up Scaleway account for on-demand testing
2. Configure GitHub Actions workflow for macOS builds
3. Document setup process for team
4. Consider self-hosted runner for heavy usage