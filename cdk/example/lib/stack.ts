import { CfnOutput, CfnParameter, Fn, Stack, StackProps, Tags } from "aws-cdk-lib";
import * as ec2 from "aws-cdk-lib/aws-ec2";
import { Construct, IConstruct } from "constructs";

// Use this to apply a `Name` tag to resources that support tagging. Check the input props for a
// `tags` field to see if a resource supports it.
function autoName<T extends IConstruct>(construct: T): T {
  Tags.of(construct).add("Name", construct.node.path);
  return construct;
}

class LinuxMachineImage implements ec2.IMachineImage {
  imageId: string;

  constructor(imageId: string) {
    this.imageId = imageId;
  }

  getImage(scope: Construct): ec2.MachineImageConfig {
    return {
      imageId: this.imageId,
      osType: ec2.OperatingSystemType.LINUX,
      userData: ec2.UserData.custom(""),
    };
  }
}

export class ExampleStack extends Stack {
  constructor(scope: Construct, id: string, props?: StackProps) {
    super(scope, id, props);

    // This is a CloudFormation parameter. The CDK recommends against using CloudFormation
    // parameters with it unless you have a good use case.
    //
    // Here is our use case: we would like to be able to deploy a new image via the Rust dropkick
    // tool without needing to have npm installed. By making this a parameter, we can update the
    // stack using the CloudFormation SDK without having to modify the CDK-generated template.
    //
    // Further reading: https://docs.aws.amazon.com/cdk/v2/guide/parameters.html
    const imageId = new CfnParameter(this, "imageId", {
      type: "AWS::EC2::Image::Id",
      description: "Dropkick image ID",
    }).valueAsString;

    const vpc = new ec2.Vpc(this, "VPC", {
      // We are creating a network interface with known IPv4 and IPv6 addresses, so we can hardcode
      // those into DNS. A network interface belongs to a subnet, and a subnet belongs to a single
      // availability zone, so we only need to configure subnets for a single availability zone. (We
      // don't have to do this, but the other subnets would not be used anyway.)
      maxAzs: 1,

      // We create two public subnets. Our instance is launched within the first subnet (which we
      // will call the Launch Subnet); our network interface with known IPv4 and IPv6 addresses
      // belongs to the second subnet (called the Service Subnet).
      //
      // EC2 instances must launch with a minimum of one (1) network interface, but we don't have a
      // good way to replace that network interface when we know the instance is ready. So we add
      // the interface with known IP addresses to the new instance, which removes it from the old
      // instance (if any).
      //
      // However, if both interfaces belong to the same subnet, responses to traffic from the second
      // interface will go out the first interface by default. This is not ideal! This class of
      // issue with multiple network interfaces in EC2 is generally dealt with by ec2-net-utils
      // (https://github.com/amazonlinux/amazon-ec2-net-utils) but instead of trying to get that to
      // work within NixOS, let's just do something less confusing (to the instance).
      //
      // "So the Launch Subnet could be an isolated private subnet?", I imagine you asking. If it
      // were, we would need to configure the instance to route default traffic out an interface
      // that doesn't exist yet on boot. It is easiest to make it public.
      subnetConfiguration: [
        { subnetType: ec2.SubnetType.PUBLIC, name: "Launch" },
        { subnetType: ec2.SubnetType.PUBLIC, name: "Service" },
      ],
    });
    const [launchSubnet, serviceSubnet] = vpc.publicSubnets as ec2.PublicSubnet[];

    // Fish the VPCGatewayAttachment out of the Vpc construct, so we can add dependencies on it.
    const vpcGatewayAttachment = vpc.node.findChild("VPCGW") as ec2.CfnVPCGatewayAttachment;

    // This allocates an AWS-provided IPv6 /56 for the VPC, which we will carve up into two /64s.
    const ipv6Block = new ec2.CfnVPCCidrBlock(this, "Ipv6Block", {
      vpcId: vpc.vpcId,
      amazonProvidedIpv6CidrBlock: true,
    });
    // Create our subnet CIDRs. This is CDK syntax sugar for CloudFormation's intrinsic functions
    // `Fn::Select` and `Fn::Cidr`, and creates two /64s.
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-select.html
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-cidr.html
    const v6cidrs = Fn.cidr(Fn.select(0, vpc.vpcIpv6CidrBlocks), 2, "64");

    // Add IPv6 CIDRs to each of our subnets. CDK's higher-level VPC constructs do not expose any
    // IPv6 support; fortunately there are escape hatches into the lower-level CloudFormation
    // constructs.
    vpc.publicSubnets.forEach((subnet, index) => {
      const cfnSubnet = subnet.node.defaultChild as ec2.CfnSubnet;
      cfnSubnet.ipv6CidrBlock = Fn.select(index, v6cidrs);
      cfnSubnet.assignIpv6AddressOnCreation = true;

      // Tell CloudFormation to delete the subnet before attempting to delete the IPv6 CIDR.
      // (Otherwise it will try to delete both simultaneously and get confused when it can't delete
      // the IPv6 CIDR due to it still being in use.)
      subnet.node.addDependency(ipv6Block);

      // The Vpc construct creates an internet gateway and routes `0.0.0.0/0` through it, but we
      // also need to route `::/0`.
      new ec2.CfnRoute(this, `${["Launch", "Service"][index]}Subnet1DefaultV6Route`, {
        routeTableId: subnet.routeTable.routeTableId,
        destinationIpv6CidrBlock: "::/0",
        gatewayId: vpc.internetGatewayId,
      }).addDependency(vpcGatewayAttachment);
    });

    // Create a security group that allows all traffic out of the instance.
    const securityGroup = autoName(
      new ec2.SecurityGroup(this, "SecurityGroup", {
        vpc,
        allowAllOutbound: true,
        allowAllIpv6Outbound: true,
      })
    );
    // Allow all ICMP/ICMPv6 traffic in from the whole internet.
    securityGroup.addIngressRule(ec2.Peer.anyIpv4(), ec2.Port.allIcmp());
    securityGroup.addIngressRule(ec2.Peer.anyIpv6(), ec2.Port.allIcmpV6());
    // Allow tcp/22 in from the whole internet.
    // FIXME make this an option (based on sshKeyName?) when this gets turned into a construct
    securityGroup.addIngressRule(ec2.Peer.anyIpv4(), ec2.Port.tcp(22));
    securityGroup.addIngressRule(ec2.Peer.anyIpv6(), ec2.Port.tcp(22));
    // Allow tcp/80 in from the whole internet. (NOTE: this is required for ACME!)
    securityGroup.addIngressRule(ec2.Peer.anyIpv4(), ec2.Port.tcp(80));
    securityGroup.addIngressRule(ec2.Peer.anyIpv6(), ec2.Port.tcp(80));
    // Allow tcp/443 in from the whole internet.
    securityGroup.addIngressRule(ec2.Peer.anyIpv4(), ec2.Port.tcp(443));
    securityGroup.addIngressRule(ec2.Peer.anyIpv6(), ec2.Port.tcp(443));

    // By default, the IPv6 address of our network interface will be randomly selected from the
    // subnet. Unfortunately there's no way to get that IP address out through CloudFormation's
    // usual methods. (we could... write a Lambda function? lol)
    //
    // Instead we will string-manipulate our way to an IPv6 address. When this interface is created,
    // the subnet is brand new, so there's no potential conflict.
    //
    // First, get the CIDR of the Service Subnet:
    const serviceCidr = Fn.select(0, serviceSubnet.subnetIpv6CidrBlocks);
    // `serviceCidr` looks like "2001:db8:123:456::/64". Remove the prefix length:
    const servicePrefix = Fn.select(0, Fn.split("/", serviceCidr));
    // `servicePrefix` looks like "2001:db8:123:456::". Append "1de":
    const serviceAddrV6 = Fn.join("", [servicePrefix, "1de"]);
    new CfnOutput(this, "DropkickServiceAddrV6", {
      value: serviceAddrV6,
      description: "Dropkick IPv6 service address (DNS AAAA record)",
    });

    const serviceInterface = autoName(
      new ec2.CfnNetworkInterface(this, "ServiceInterface", {
        subnetId: serviceSubnet.subnetId,
        groupSet: [securityGroup.securityGroupId],
        ipv6Addresses: [{ ipv6Address: serviceAddrV6 }],
      })
    );

    // The only way to add a public IPv4 address to a network interface that stays consistent
    // between instance stop-starts is an Elastic IP ("EIP" in CloudFormation lingo).
    const serviceAddrV4 = autoName(new ec2.CfnEIP(this, "EIP"));
    new CfnOutput(this, "DropkickServiceAddrV4", {
      value: serviceAddrV4.attrPublicIp,
      description: "Dropkick IPv4 service address (DNS A record)",
    });
    new ec2.CfnEIPAssociation(this, "EIPAssociation", {
      allocationId: serviceAddrV4.attrAllocationId,
      networkInterfaceId: serviceInterface.ref,
    });

    const instance = new ec2.Instance(this, "Instance", {
      instanceType: new ec2.InstanceType("t3.medium"),
      machineImage: new LinuxMachineImage(imageId),
      vpc,
      keyName: "iliana@redwood", // FIXME
      requireImdsv2: true,
      securityGroup,
      vpcSubnets: { subnets: [launchSubnet] },
    });

    new ec2.CfnNetworkInterfaceAttachment(this, "InterfaceAttachment", {
      deleteOnTermination: false,
      deviceIndex: "1",
      instanceId: instance.instanceId,
      networkInterfaceId: serviceInterface.ref,
    });
  }
}
