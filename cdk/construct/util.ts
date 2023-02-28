import { Tags } from "aws-cdk-lib";
import * as ec2 from "aws-cdk-lib/aws-ec2";
import { IConstruct } from "constructs";

/**
 * Use this to apply a `Name` tag to resources that support tagging. Check the
 * input props for a `tags` field to see if a resource supports it.
 */
export function autoName<T extends IConstruct>(construct: T): T {
  Tags.of(construct).add("Name", construct.node.path);
  return construct;
}

/**
 * Simple ec2.IMachineImage implementation where we know the image ID is
 * supposed to be valid in our current region.
 */
export class LinuxMachineImage implements ec2.IMachineImage {
  imageId: string;

  constructor(imageId: string) {
    this.imageId = imageId;
  }

  getImage(): ec2.MachineImageConfig {
    return {
      imageId: this.imageId,
      osType: ec2.OperatingSystemType.LINUX,
      userData: ec2.UserData.custom(""),
    };
  }
}
