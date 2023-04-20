# Deploying in AWS

We recommend deploying your project with the AWS CDK and the [`@oxide/dropkick-cdk`](https://www.npmjs.com/package/@oxide/dropkick-cdk) construct. These setup steps require you have Node.js and npm installed.

## Creating an EC2 image

Get some AWS credentials, then run `dropkick create-ec2-image`:

```bash
dropkick create-ec2-image --hostname beachball.example ~/git/beachball
```

If all goes well, you'll get an EC2 image ID (e.g. `ami-0987654321example`). You'll use that in a future step.

## Setting up your project

Create an empty CDK project. (We're reusing the `beachball` name from [getting-started.md](./getting-started.md); you can use whatever name you like.)

```bash
mkdir beachball-cdk
cd beachball-cdk
npx aws-cdk init --language typescript
```

Install @oxide/dropkick-cdk:

```bash
npm install @oxide/dropkick-cdk
```

Use the dropkick construct in your stack by editing `lib/beachball-cdk-stack.ts` (or whatever your stack is called):

```diff
 import * as cdk from 'aws-cdk-lib';
 import { Construct } from 'constructs';
+import * as ec2 from 'aws-cdk-lib/aws-ec2';
+import { DropkickInstance } from '@oxide/dropkick-cdk';

 export class BeachballStack extends cdk.Stack {
   constructor(scope: Construct, id: string, props?: cdk.StackProps) {
     super(scope, id, props);
+
+    const instance = new DropkickInstance(this, "Instance", {
+      instanceType: new ec2.InstanceType("t3.medium"),
+    })
   }
 }
```

> **Note**
> If you want SSH access to your instance, you will need to run ``

## First deployment

The CDK needs to create bootstrap stacks if your account and region don't have them yet, so do that first:

```bash
npm run cdk bootstrap
```

Then deploy:

```bash
npm run cdk deploy -- --parameters DropkickImageId=ami-0987654321example
```

(Note the `--` is load-bearing; otherwise npm will think the `--parameters` option is for it!)

## Configure DNS

When the stack is deployed, you will see these output variables:

```
Outputs:
BeachballStack.DropkickServicePublicIpv4 = 198.51.100.123
BeachballStack.DropkickServicePublicIpv6 = 2001:db8:9951:400::1de
```

Use these to set up A and AAAA DNS records for your hostname (in our example, `beachball.example`).

Once these records are propagated, reaching your service over HTTPS should work!

## Next deployments

As you make changes to your CDK stack, you can deploy those changes with `npm run cdk deploy`.

If you want to update only the image in the stack, the dropkick command line tool can do this for you:

```bash
dropkick deploy-ec2-image --hostname beachball.example ~/git/beachball BeachballStack
```

where `BeachballStack` is the CloudFormation stack name the CDK deployed.
