// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { CfnOutput, CfnParameter, Fn, Stack } from "aws-cdk-lib";
import * as ec2 from "aws-cdk-lib/aws-ec2";
import * as iam from "aws-cdk-lib/aws-iam";
import { Construct } from "constructs";
import { HealthCheck } from "./health";
import { autoName, LinuxMachineImage } from "./util";

/**
 * Configuration for DropkickInstance
 */
export interface DropkickInterfaceProps {
  instanceRoleName?: string;
  instanceRolePath?: string;
  instanceType: ec2.InstanceType;
  sshKeyName?: string;
}

/**
 * Create a VPC and EC2 instance from an AMI built by Dropkick
 */
export class DropkickInstance extends Construct {
  // Provide an interface to the resources we've created, so stacks can hook
  // into them (e.g. adding additional resources to the VPC or subnet, or
  // authorizing the instance role to do things).
  instance: ec2.Instance;
  instanceRole: iam.IRole;
  servicePublicIpv4: string;
  servicePublicIpv6: string;
  serviceSecurityGroup: ec2.SecurityGroup;
  serviceSubnet: ec2.PublicSubnet;
  vpc: ec2.Vpc;

  constructor(stack: Stack, id: string, props: DropkickInterfaceProps) {
    super(stack, id);

    // This is a CloudFormation parameter. The CDK recommends against using
    // CloudFormation parameters with it unless you have a good use case.
    //
    // Here is our use case: we would like to be able to deploy a new image via
    // the Rust dropkick tool without needing to have npm installed. By making
    // this a parameter, we can update the stack using the CloudFormation SDK
    // without having to modify the CDK-generated template.
    //
    // Further reading: https://docs.aws.amazon.com/cdk/v2/guide/parameters.html
    const imageId = new CfnParameter(stack, "DropkickImageId", {
      type: "AWS::EC2::Image::Id",
      description: "Dropkick image ID",
    }).valueAsString;

    this.vpc = new ec2.Vpc(this, "VPC", {
      // We only *need* one subnet because we are creating a network interface
      // with known IPv4 and IPv6 addresses, so we can hardcode those into DNS.
      // A network interface belongs to a subnet, and a subnet belongs to a
      // single availability zone, so we only need to configure subnets for a
      // single availability zone. But unfortunately systems like RDS assume you
      // have subnets in more than one AZ. This sucks, but what're you gonna do.
      maxAzs: 2,

      subnetConfiguration: [
        // We define the "Service" subnet as the subnet containing the network
        // interface whose IP addresses are in DNS for our service. Currently,
        // we also use this subnet for the "launch interface" (the primary
        // interface on the instance) because we don't need a separate subnet
        // for it.
        { subnetType: ec2.SubnetType.PUBLIC, name: "Service" },

        // We also set up an isolated private subnet, which is useful for things
        // like databases or whatever. Not used in this stack, but things using
        // this same VPC might want 'em.
        { subnetType: ec2.SubnetType.PRIVATE_ISOLATED, name: "Isolated" },
      ],
    });
    // Fish the VPCGatewayAttachment out of the Vpc construct, so we can add
    // dependencies on it.
    const vpcgw = this.vpc.node.findChild(
      "VPCGW"
    ) as ec2.CfnVPCGatewayAttachment;

    // The Vpc construct carves up all of the available IPv4 space across its
    // subnets. This means adding new subnets in the future will require
    // renumbering, which requires replacement. Not ideal! Let's do some
    // sensible numbering: a /20 for each.
    const allSubnets = [
      ...this.vpc.publicSubnets,
      ...this.vpc.privateSubnets,
      ...this.vpc.isolatedSubnets,
    ];
    // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/intrinsic-function-reference-cidr.html
    const v4cidrs = Fn.cidr(
      this.vpc.vpcCidrBlock,
      allSubnets.length,
      // I want to know what was going through the mind of the engineer who
      // decided this parameter should be the suffix length of the CIDR and not
      // the prefix length.
      (32 - 20).toString()
    );
    allSubnets.forEach((subnet, i) => {
      (subnet.node.defaultChild as ec2.CfnSubnet).cidrBlock = Fn.select(
        i,
        v4cidrs
      );
    });

    // Allocate an AWS-provided IPv6 /56 for the VPC.
    const ipv6Block = new ec2.CfnVPCCidrBlock(this, "Ipv6Block", {
      vpcId: this.vpc.vpcId,
      amazonProvidedIpv6CidrBlock: true,
    });
    // Carve that /56 up into a /64 for each public subnet.
    const v6cidrs = Fn.cidr(
      Fn.select(0, this.vpc.vpcIpv6CidrBlocks),
      this.vpc.publicSubnets.length,
      (128 - 64).toString()
    );
    // Add IPv6 CIDRs to each of our subnets.
    this.vpc.publicSubnets.forEach((subnet, i) => {
      const cfnSubnet = subnet.node.defaultChild as ec2.CfnSubnet;
      cfnSubnet.ipv6CidrBlock = Fn.select(i, v6cidrs);
      cfnSubnet.assignIpv6AddressOnCreation = true;

      // Tell CloudFormation to delete the subnet before attempting to delete
      // the IPv6 CIDR. (Otherwise it will try to delete both simultaneously and
      // get confused when it can't delete the IPv6 CIDR due to it still being
      // in use.)
      subnet.node.addDependency(ipv6Block);

      // The Vpc construct creates an internet gateway and routes `0.0.0.0/0`
      // through it, but we also need to route `::/0`.
      new ec2.CfnRoute(subnet, "DefaultRouteV6", {
        routeTableId: subnet.routeTable.routeTableId,
        destinationIpv6CidrBlock: "::/0",
        gatewayId: this.vpc.internetGatewayId,
      }).addDependency(vpcgw);
    });

    this.serviceSubnet = this.vpc.publicSubnets[0] as ec2.PublicSubnet;

    // Create a security group that allows all traffic out of the instance.
    this.serviceSecurityGroup = autoName(
      new ec2.SecurityGroup(this, "SecurityGroup", {
        vpc: this.vpc,
        allowAllOutbound: true,
        allowAllIpv6Outbound: true,
      })
    );
    // Allow all ICMP/ICMPv6 traffic in from the whole internet.
    this.serviceSecurityGroup.addIngressRule(
      ec2.Peer.anyIpv4(),
      ec2.Port.allIcmp()
    );
    this.serviceSecurityGroup.addIngressRule(
      ec2.Peer.anyIpv6(),
      ec2.Port.allIcmpV6()
    );
    [ec2.Peer.anyIpv4(), ec2.Peer.anyIpv6()].forEach((peer) => {
      // Allow tcp/22 for SSH if we are setting an SSH key name.
      if (props.sshKeyName !== undefined) {
        this.serviceSecurityGroup.addIngressRule(peer, ec2.Port.tcp(22));
      }
      // Allow tcp/80 for HTTP from the whole internet (required for ACME!).
      this.serviceSecurityGroup.addIngressRule(peer, ec2.Port.tcp(80));
      // Allow tcp/443 for HTTPS from the whole internet.
      this.serviceSecurityGroup.addIngressRule(peer, ec2.Port.tcp(443));
    });

    // By default, the IPv6 address of our network interface will be randomly
    // selected from the subnet. Unfortunately there's no way to get that IP
    // address out through CloudFormation's usual methods. (we could... write a
    // Lambda function? lol)
    //
    // Instead we will string-manipulate our way to an IPv6 address. When this
    // interface is created, the subnet is brand new, so there's no potential
    // conflict.
    //
    // First, get the CIDR of the Service Subnet:
    const serviceCidr = Fn.select(0, this.serviceSubnet.subnetIpv6CidrBlocks);
    // `serviceCidr` looks like "2001:db8:123:456::/64". Remove the prefix length:
    const servicePrefix = Fn.select(0, Fn.split("/", serviceCidr));
    // `servicePrefix` looks like "2001:db8:123:456::". Append "1de":
    this.servicePublicIpv6 = Fn.join("", [servicePrefix, "1de"]);
    new CfnOutput(stack, "DropkickServicePublicIpv6", {
      value: this.servicePublicIpv6,
      description: "Dropkick IPv6 service address (DNS AAAA record)",
    });

    const serviceInterface = autoName(
      new ec2.CfnNetworkInterface(this, "ServiceInterface", {
        subnetId: this.serviceSubnet.subnetId,
        groupSet: [this.serviceSecurityGroup.securityGroupId],
        ipv6Addresses: [{ ipv6Address: this.servicePublicIpv6 }],
      })
    );

    // The only way to add a public IPv4 address to a network interface that
    // stays consistent between instance stop-starts is an Elastic IP ("EIP" in
    // CloudFormation lingo).
    const serviceAddrV4 = autoName(new ec2.CfnEIP(this, "EIP"));
    this.servicePublicIpv4 = serviceAddrV4.attrPublicIp;
    new CfnOutput(stack, "DropkickServicePublicIpv4", {
      value: this.servicePublicIpv4,
      description: "Dropkick IPv4 service address (DNS A record)",
    });
    new ec2.CfnEIPAssociation(this, "EIPAssociation", {
      allocationId: serviceAddrV4.attrAllocationId,
      networkInterfaceId: serviceInterface.ref,
    });

    // Create an instance role in the same way `new ec2.Instance()` does, but
    // expose the name and path parameters from `props`.
    const role =
      props.instanceRoleName || props.instanceRolePath
        ? new iam.Role(this, "InstanceRole", {
            assumedBy: new iam.ServicePrincipal("ec2.amazonaws.com"),
            path: props.instanceRolePath,
            roleName: props.instanceRoleName,
          })
        : undefined;

    this.instance = autoName(
      new ec2.Instance(this, "Resource", {
        instanceType: props.instanceType,
        machineImage: new LinuxMachineImage(imageId),
        role,
        vpc: this.vpc,
        keyName: props.sshKeyName,
        requireImdsv2: true,
        securityGroup: this.serviceSecurityGroup,
        vpcSubnets: { subnets: [this.serviceSubnet] },
      })
    );
    this.instanceRole = this.instance.role;

    const healthCheck = new HealthCheck(this, "HealthCheck", {
      publicIp: this.instance.instancePublicIp,
    });
    const attachment = new ec2.CfnNetworkInterfaceAttachment(
      this,
      "InterfaceAttachment",
      {
        deleteOnTermination: false,
        deviceIndex: "1",
        instanceId: this.instance.instanceId,
        networkInterfaceId: serviceInterface.ref,
      }
    );
    attachment.node.addDependency(healthCheck);
  }
}
