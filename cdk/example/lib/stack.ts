import { DropkickInstance } from "@oxide/dropkick-cdk";
import { Stack, StackProps } from "aws-cdk-lib";
import { InstanceType } from "aws-cdk-lib/aws-ec2";
import { Construct } from "constructs";

export class ExampleStack extends Stack {
  constructor(scope: Construct, id: string, props?: StackProps) {
    super(scope, id, props);

    new DropkickInstance(this, "DropkickInstance", {
      instanceType: new InstanceType("t3.medium"),
    });
  }
}
