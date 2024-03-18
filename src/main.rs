use base64::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

mod sdk_wrapper;

#[allow(deprecated)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up AWS environment and client
    let shared_config = aws_config::load_from_env().await;
    let client: aws_sdk_ec2::Client = aws_sdk_ec2::Client::new(&shared_config);

    // Get user data from the script file and encode
    let user_data: String = {
        let user_data_path = Path::new("./src/user_data.sh");

        match fs::read_to_string(user_data_path) {
            Ok(data) => BASE64_STANDARD.encode(data.as_bytes()),
            Err(err) => {
                panic!(
                    "Failed to read user data file at path {:#?}: {:#?}",
                    user_data_path, err
                )
            }
        }
    };

    // Attmempt to create instances by iterating through AZs
    let azs = vec!["us-west-2a", "us-west-2b", "us-west-2c"];
    for i in 0..1024 {
        // Try an AZ
        let az = azs[i % 3];
        println!("\n--- Attemtping to create instance in AZ: {} ---", az);

        // Set up template
        let template = sdk_wrapper::InstanceTemplate {
            availability_zone: az,
            ami_image_id: "ami-0c57248507328e2de", // AMI Name: sc24-nccl-experiments-v2
            instance_type: aws_sdk_ec2::types::InstanceType::G52xlarge,
            subnet_id: "subnet-005f41c66eb78bc89",
            security_group_id: "sg-0fa33c632d08f14ea",
            user_data: Some(&user_data),
            num_ifaces: 4,
            use_efa: false,
            project_tag: "test",
        };

        // Try to create the instance(s)
        match sdk_wrapper::create_instance_sdk(&client, &template).await {
            Ok(_) => {
                println!(
                    "🎉🎉🎉🎉 Successfully created instance(s) in AZ: {} 🎉🎉🎉🎉",
                    az
                );
                break;
            }
            Err(e) => {
                println!("Failed to create instance(s) in AZ {}: {:#?}", az, e);
            }
        }

        // Sleep so as not to activate a rate limit
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }

    Ok(())
}

#[tokio::test]
async fn setup_aws() -> Result<(), Box<dyn std::error::Error>> {
    // Set up AWS environment and client
    let shared_config = aws_config::load_from_env().await;
    let client: aws_sdk_ec2::Client = aws_sdk_ec2::Client::new(&shared_config);

    // Print info about the client
    print!("{:#?}", client.describe_instances().send().await?);

    // Just test
    println!("Done testing.");

    Ok(())
}

#[tokio::test]
async fn try_create_instance() -> Result<(), Box<dyn std::error::Error>> {
    // Set up AWS environment and client
    let shared_config = aws_config::load_from_env().await;
    let client: aws_sdk_ec2::Client = aws_sdk_ec2::Client::new(&shared_config);

    // Set up variables
    let availability_zone = "us-west-2a";
    let mut subnets: HashMap<&str, &str> = HashMap::new();
    subnets.insert("us-west-2a", "subnet-005f41c66eb78bc89");
    subnets.insert("us-west-2b", "subnet-0fdc25184c15c4a13");
    subnets.insert("us-west-2c", "subnet-08e9d9fc4f61bf02d");
    let chosen_subnet: &str = subnets.get(availability_zone).unwrap();

    let security_group_id = "sg-0fa33c632d08f14ea";

    // Set up template
    let template = sdk_wrapper::InstanceTemplate {
        availability_zone: "us-west-2a",
        ami_image_id: "ami-07bff6261f14c3a45", // AMI Name: sc24-nccl-experiments-v2
        instance_type: aws_sdk_ec2::types::InstanceType::T2Micro,
        subnet_id: chosen_subnet,
        security_group_id: security_group_id,
        user_data: None,
        num_ifaces: 1,
        use_efa: false,
        project_tag: "testing_sdk",
    };

    // Try to create the instance(s)
    let instances = sdk_wrapper::create_instance_sdk(&client, &template).await?;
    println!("Successfully created the instance. Will attempt to destroy shortly (please wait).");

    // Wait a few seconds
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    println!("Attemtping to destroy the instance.");

    // Build vector of instance IDs
    let instance_ids: Vec<_> = instances
        .iter()
        .map(|instance| instance.instance_id().unwrap().to_string())
        .collect();

    // Try to tear down the instance(s)
    sdk_wrapper::terminate_instances(&client, instance_ids).await?;

    Ok(())
}

#[tokio::test]
async fn try_create_cluster() -> Result<(), Box<dyn std::error::Error>> {
    // Set up AWS environment and client
    let shared_config = aws_config::load_from_env().await;
    let client: aws_sdk_ec2::Client = aws_sdk_ec2::Client::new(&shared_config);

    // Set up the cluster template
    let cluster_template = sdk_wrapper::ClusterTemplate {
        cluster_name: "SDK Testing Cluster",
        num_instances: 1,
        instance_template: sdk_wrapper::InstanceTemplate {
            availability_zone: "us-west-2a",
            ami_image_id: "ami-07bff6261f14c3a45", // AMI Name: sc24-nccl-experiments-v2
            instance_type: aws_sdk_ec2::types::InstanceType::T2Micro,
            subnet_id: "subnet-005f41c66eb78bc89",
            security_group_id: "sg-0fa33c632d08f14ea",
            user_data: None,
            num_ifaces: 1,
            use_efa: false,
            project_tag: "testing_sdk",
        },
        attach_shared_ebs: false,
        shared_ebs_volume_size: None,
        project_tag: "testing_sdk",
    };
    println!(
        "Will attempt to create a cluster with the following template: {:#?}",
        cluster_template
    );

    // Create a VPC for the cluster
    let (vpc_id, vpc_cleanup) = sdk_wrapper::create_vpc(
        &client,
        cluster_template.cluster_name,
        cluster_template.project_tag,
    )
    .await?;
    println!(
        "Created VPC with ID: {}. Can clean up with: {:#?}",
        vpc_id, vpc_cleanup
    );

    // Create the Cluster
    // TODO: Implement
    println!("[WARN] Cluster creation not yet implemented!");

    // Wait a few seconds
    let wait_seconds = 15;
    println!(
        "Waiting {} second(s) before tearing down the cluster and associated entities.",
        wait_seconds
    );
    tokio::time::sleep(tokio::time::Duration::from_secs(wait_seconds)).await;
    println!("Done waiting. Will now tear down the cluster and associated entities.");

    // Tear down the cluster
    // TODO: Implement
    println!("[WARN] Cluster teardown not yet implemented!");

    // Tear down the VPC
    sdk_wrapper::cleanup_vpc(&client, vpc_cleanup).await?;

    // Return success
    Ok(())
}
