import * as cdk from 'aws-cdk-lib';
import { Construct } from 'constructs';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import { DropkickInstance } from '@oxide/dropkick-cdk';

export class CdkStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    new DropkickInstance(this, "Instance", {
      instanceType: new ec2.InstanceType("t3.micro"),
    });
  }
}
