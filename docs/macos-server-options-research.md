# macOS Server Options for Apple Review Testing

## Research Summary (January 31, 2026)

### Context
For iOS app review and testing, we need access to macOS servers to run builds, tests, and simulate the Apple review environment.

### Current Options

#### 1. Scaleway Mac mini M1
- **URL**: https://www.scaleway.com/en/hello-m1/
- **Hardware**: Apple M1 chip (8-core CPU, 8-core GPU, 16-core Neural Engine)
- **Memory**: 8GB LPDDR4
- **Storage**: 256GB SSD
- **Network**: 1 Gbps bandwidth
- **OS**: Latest macOS versions (Sonoma 14 by default)
- **Access**: VNC (screen sharing) or SSH
- **Location**: Paris datacenter (excellent latency from Europe)
- **Pricing**: Hourly billing after minimum 24-hour allocation
- **Key Feature**: Well-integrated with CI/CD workflows

#### 2. OakHost Mac mini Hosting
- **URL**: https://www.oakhost.net/mac-mini-hosting
- **Note**: Servers are hosted at Hetzner datacenters
- **Advantage**: Fast/free traffic to other Hetzner servers
- **Pricing**: Monthly plans available

#### 3. MacinCloud (Alternative)
- More expensive than Scaleway for on-demand usage
- Better suited for long-term monthly commitments

### Historical Note: Hetzner Mac mini
- Hetzner previously offered Mac mini hosting but discontinued the service
- Many users migrated to OakHost (which uses Hetzner datacenters) or Scaleway

### Recommendation for MobileCLI Testing

**For Apple Review Testing**: Scaleway Mac mini M1
- Reasons:
  - Hourly billing makes it cost-effective for temporary testing
  - Direct SSH/VNC access for debugging
  - Latest macOS versions ensure compatibility
  - 24-hour minimum is reasonable for review testing cycles
  - European location good for global access

**For Long-term Development**: Consider OakHost
- Better if you already use Hetzner infrastructure
- Monthly pricing may be more economical for continuous use

### Setup Process
1. Sign up for Scaleway account
2. Create Mac mini M1 instance
3. Connect via SSH or VNC
4. Install Xcode and development tools
5. Clone MobileCLI repository
6. Run build and tests
7. Deploy to TestFlight/App Store Connect

### Cost Optimization Tips
- Spin up instances only when needed for testing
- Automate build processes to minimize runtime
- Use for final testing before App Store submission
- Delete instances after 24-hour minimum period