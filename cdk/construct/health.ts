// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { CustomResource, Duration, Resource } from "aws-cdk-lib";
import * as iam from "aws-cdk-lib/aws-iam";
import * as lambda from "aws-cdk-lib/aws-lambda";
import * as nodejs from "aws-cdk-lib/aws-lambda-nodejs";
import { Construct } from "constructs";

export interface HealthCheckProps {
  publicIp: string;
}

export class HealthCheck extends Resource {
  constructor(scope: Construct, id: string, props: HealthCheckProps) {
    super(scope, id);

    const handler = new nodejs.NodejsFunction(this, "handler", {
      // Node 18.x or higher is required for fetch().
      runtime: lambda.Runtime.NODEJS_22_X,

      // The function stops trying to ping after 10 minutes; this timeout needs
      // to be higher so we have time to post a failed message if we reach that
      // point.
      timeout: Duration.minutes(15),

      // This role deliberately has no permissions to disable emitting execution
      // logs to CloudWatch Logs, so that we don't have to set up a retention
      // policy (which uses another Lambda function! wow!)
      role: new iam.Role(this, "ServiceRole", {
        assumedBy: new iam.ServicePrincipal("lambda.amazonaws.com"),
      }),
    });

    new CustomResource(this, "Resource", {
      resourceType: "Custom::DropkickHealthCheck",
      serviceToken: handler.functionArn,
      properties: {
        PublicIp: props.publicIp,
      },
    });
  }
}
