import { CfnOutput, CfnParameter, Fn, Stack, StackProps, Tags } from "aws-cdk-lib";
import * as ec2 from "aws-cdk-lib/aws-ec2";
import { Construct, IConstruct } from "constructs";

// Use this to apply a `Name` tag to resources that support tagging. Check the input props for a
// `tags` field to see if a resource supports it.
function autoName<T extends IConstruct>(construct: T): T {
  Tags.of(construct).add("Name", construct.node.path);
  return construct;
}

export class ExampleStack extends Stack {
  constructor(scope: Construct, id: string, props?: StackProps) {
    super(scope, id, props);

    const imageId = new CfnParameter(this, "imageId", {
      type: "AWS::EC2::Image::Id",
      description: "Dropkick image ID",
    });

    const vpc = new ec2.Vpc(this, "VPC", {
      enableDnsHostnames: false,

      // disable automatic creation of subnets and other resources
      subnetConfiguration: [],
    });

    // This allocates an IPv6 /56 in an AWS-provided space.
    const ipv6Block = new ec2.CfnVPCCidrBlock(this, "Ipv6Block", {
      vpcId: vpc.vpcId,
      amazonProvidedIpv6CidrBlock: true,
    });

    // Create our subnet CIDRs. This is CDK syntax sugar for CloudFormation's intrinsic functions
    // `Fn::Select` and `Fn::Cidr`.
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-select.html
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-cidr.html
    const v4cidr = Fn.select(0, Fn.cidr(vpc.vpcCidrBlock, 1, "12"));
    const v6cidr = Fn.select(0, Fn.cidr(Fn.select(0, vpc.vpcIpv6CidrBlocks), 1, "64"));

    const subnet = new ec2.PublicSubnet(this, "Subnet", {
      vpcId: vpc.vpcId,
      cidrBlock: v4cidr,
      availabilityZone: Stack.of(this).availabilityZones[0],
      mapPublicIpOnLaunch: true,
    });
    // CDK's higher-level constructs do not expose any IPv6 support. Fortunately there are escape
    // hatches into the lower-level CloudFormation constructs.
    (subnet.node.defaultChild as ec2.CfnSubnet).ipv6CidrBlock = v6cidr;
    (subnet.node.defaultChild as ec2.CfnSubnet).assignIpv6AddressOnCreation = true;
    // Tell CloudFormation to delete the subnet before attempting to delete the IPv6 CIDR.
    // (Otherwise it will try to delete both simultaneously and get confused when it can't delete
    // the IPv6 CIDR due to it still being in use.)
    subnet.node.addDependency(ipv6Block);

    // Set up an internet gateway.
    const gateway = autoName(new ec2.CfnInternetGateway(this, "Gateway"));
    const attachment = new ec2.CfnVPCGatewayAttachment(this, "GatewayAttachment", {
      vpcId: vpc.vpcId,
      internetGatewayId: gateway.ref,
    });
    // Set up our default routes to the internet gateway.
    new ec2.CfnRoute(this, "DefaultV4Route", {
      routeTableId: subnet.routeTable.routeTableId,
      destinationCidrBlock: "0.0.0.0/0",
      gatewayId: gateway.ref,
    }).addDependency(attachment);
    new ec2.CfnRoute(this, "DefaultV6Route", {
      routeTableId: subnet.routeTable.routeTableId,
      destinationIpv6CidrBlock: "::/0",
      gatewayId: gateway.ref,
    }).addDependency(attachment);

    const securityGroup = autoName(
      new ec2.SecurityGroup(this, "SecurityGroup", {
        vpc,
        // FIXME make this an option when this gets turned into a construct
        allowAllOutbound: true,
        allowAllIpv6Outbound: true,
      })
    );
    securityGroup.addIngressRule(ec2.Peer.anyIpv4(), ec2.Port.icmpPing());
    securityGroup.addIngressRule(
      // ICMPv6 echo request
      ec2.Peer.anyIpv6(),
      new ec2.Port({
        protocol: ec2.Protocol.ICMPV6,
        fromPort: 128,
        toPort: -1,
        stringRepresentation: `ICMPv6 Type 128`,
      })
    );
    [ec2.Peer.anyIpv4(), ec2.Peer.anyIpv6()].forEach((peer) => {
      securityGroup.addIngressRule(peer, ec2.Port.tcp(22));
      securityGroup.addIngressRule(peer, ec2.Port.tcp(80));
      securityGroup.addIngressRule(peer, ec2.Port.tcp(443));
    });

    // There's no attribute to get an IPv6 address of a network interface, so we'll make one. This
    // is equivalent to:
    //
    // > '2001:db8:12:34:0:0:0:0/64'.split(':0:0:0:0/64')[0] + '::1de'
    // '2001:db8:12:34::1de'
    //
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-join.html
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-select.html
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-split.html
    const v6addr = Fn.join("", [Fn.select(0, Fn.split(":0:0:0:0/64", v6cidr)), "::1de"]);

    const networkInterface = autoName(
      new ec2.CfnNetworkInterface(this, "NetworkInterface", {
        subnetId: subnet.subnetId,
        groupSet: [securityGroup.securityGroupId],
        ipv6Addresses: [{ ipv6Address: v6addr }],
      })
    );

    const v4addr = autoName(new ec2.CfnEIP(this, "EIP"));
    new ec2.CfnEIPAssociation(this, "EIPAssociation", {
      allocationId: v4addr.attrAllocationId,
      networkInterfaceId: networkInterface.ref,
    });

    new CfnOutput(this, "dropkickIPv4", {
      value: v4addr.attrPublicIp,
      description: "Public IPv4 address (A record)",
    });
    new CfnOutput(this, "dropkickIPv6", {
      value: v6addr,
      description: "Public IPv6 address (AAAA record)",
    });
  }
}
