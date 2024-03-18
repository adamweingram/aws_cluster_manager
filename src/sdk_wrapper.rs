use aws_sdk_ec2::error::ProvideErrorMetadata;
use aws_sdk_ec2::types::Instance;
use aws_sdk_ec2::Client;
use aws_sdk_ec2::{error::SdkError, types};
use base64::prelude::*;
use termion::{color, style};

#[derive(Debug, Clone)]
pub struct InstanceTemplate<'a> {
    pub availability_zone: &'a str,
    pub ami_image_id: &'a str,
    pub instance_type: types::InstanceType,
    pub subnet_id: &'a str,
    pub security_group_id: &'a str,
    pub num_ifaces: u64,
    pub use_efa: bool,
    pub user_data: Option<&'a str>,
    pub project_tag: &'a str,
}

pub async fn create_instance_sdk<'a>(
    aws_client: &aws_sdk_ec2::Client,
    template: &InstanceTemplate<'a>,
) -> Result<Vec<Instance>, Box<dyn std::error::Error>> {
    // let placement_group_id = "pg-026f038784dd1240b";

    // // Create a block device for the instance
    // let block_dev = types::BlockDeviceMapping::builder()
    //     .device_name("/dev/xvda")
    //     .ebs(types::EbsBlockDevice::builder()
    //         .encrypted(false)
    //         .delete_on_termination(true)
    //         .snapshot_id("snap-0baf9c142cfc1ea82")
    //         .volume_size(1024)
    //         .volume_type(types::VolumeType::Gp3)
    //         .build())
    //     .build();

    // Create network interfaces
    let mut network_interfaces = Vec::new();
    for i in 0..template.num_ifaces {
        if !template.use_efa {
            let net_iface = types::InstanceNetworkInterfaceSpecification::builder()
                .subnet_id(template.subnet_id)
                .delete_on_termination(true)
                .associate_public_ip_address(if i == 0 { true } else { false })
                .device_index(i.try_into()?)
                .network_card_index(i.try_into()?)
                .groups(template.security_group_id)
                // TODO: MAKE THIS AN EFA, NOT A STANDARD INTERFACE!!!
                .build();
            network_interfaces.push(net_iface);
        } else {
            let net_iface = types::InstanceNetworkInterfaceSpecification::builder()
                .subnet_id(template.subnet_id)
                .delete_on_termination(true)
                .associate_public_ip_address(if i == 0 { true } else { false })
                .device_index(i.try_into()?)
                .network_card_index(i.try_into()?)
                .groups(template.security_group_id)
                .build();
            network_interfaces.push(net_iface);

            panic!("Only EFAs are supported as network interfaces right now!");
        }
    }

    // let net_iface = types::InstanceNetworkInterfaceSpecification::builder()
    //     .subnet_id(chosen_subnet)
    //     .delete_on_termination(true)
    //     .associate_public_ip_address(true)
    //     .device_index(0)
    //     .interface_type("efa")
    //     .network_card_index(0)
    //     .groups(security_group_id)
    //     .build();

    // Create the instance with specific options
    let mut run_instance_builder = aws_client
        .run_instances()
        .key_name("adam-hwkey")
        .image_id(template.ami_image_id)
        .instance_type(template.instance_type.clone())
        .disable_api_termination(false)
        .set_network_interfaces(Some(network_interfaces)) // Using `set` here overrides any other usages of `network_interfaces` in the builder
        // .block_device_mappings(block_dev)
        .min_count(1)
        .max_count(1)
        .tag_specifications(
            types::TagSpecification::builder()
                .resource_type(types::ResourceType::Instance)
                .tags(
                    types::Tag::builder()
                        .key("project")
                        .value(template.project_tag)
                        .build(),
                )
                .build(),
        )
        .iam_instance_profile(
            types::IamInstanceProfileSpecification::builder()
                .arn("arn:aws:iam::703446475099:instance-profile/ec2-aws-access")
                .build(),
        )
        // .placement(types::Placement::builder()
        //     .group_id(placement_group_id)
        //     .build())
        .metadata_options(
            types::InstanceMetadataOptionsRequest::builder()
                .http_tokens(types::HttpTokensState::Optional)
                .http_endpoint(types::InstanceMetadataEndpointState::Enabled)
                .build(),
        );

    // Add user data only if it is given
    run_instance_builder = match template.user_data {
        Some(user_data) => {
            let user_data = user_data.as_bytes();
            let user_data_b64 = BASE64_STANDARD.encode(user_data);
            run_instance_builder.user_data(user_data_b64)
        }
        None => run_instance_builder,
    };

    let run_instances_req = run_instance_builder.send().await;

    // Handle the result of the attempted instance creation
    let instances = match run_instances_req {
        Ok(val) => {
            println!("Creation did not error. Checking for instance...");
            let instances = match val.instances {
                Some(instances) => instances,
                None => {
                    println!("No instances were returned in the response!");
                    return Err("No instances were returned in the response!".into());
                }
            };
            println!("Successfully created instance: {:#?}", instances);

            if instances.len() != 1 {
                println!("[WARNING] Expected to create a single instance, but created {} instead! This is probably a serious problem with the tool; report immediately!", instances.len());
            }

            instances
        }
        Err(err) => match &err {
            SdkError::ServiceError(_) => {
                match err.meta().code() {
                    Some(code) => {
                        println!("Got ServiceError with code: {}", code);
                        return Err(Box::new(err));
                    }
                    None => {
                        println!("Got ServiceError without code: {:#?}", err);
                        return Err(Box::new(err));
                    }
                };
            }
            _ => {
                println!("Got some other error");
                return Err(Box::new(err));
            }
        },
    };

    // Return a vector of instances. Should only contain a single value!
    Ok(instances)
}

#[derive(Debug, Clone)]
pub struct ClusterTemplate<'a> {
    pub cluster_name: &'a str,
    pub num_instances: u64,
    pub instance_template: InstanceTemplate<'a>,
    pub attach_shared_ebs: bool,
    pub shared_ebs_volume_size: Option<u64>,
    pub project_tag: &'a str,
}

/// Create a cluster of instances based on a template.
pub async fn create_cluster<'a>(
    aws_client: &aws_sdk_ec2::Client,
    template: &ClusterTemplate<'a>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Verify settings
    if template.num_instances < 1 {
        return Err("Number of instances must be at least 1!".into());
    } else if template.num_instances > 16 && template.attach_shared_ebs {
        return Err("Cannot attach shared EBS volume to more than 16 instances!".into());
    }

    if template.project_tag != template.instance_template.project_tag {
        return Err(
            "Project tag in cluster template must match project tag in instance template!".into(),
        );
    }

    if template.attach_shared_ebs && template.shared_ebs_volume_size.is_none() {
        return Err("If attaching shared EBS volume, must specify its size!".into());
    }

    // Create the shared block storage
    let ebs_vol_id = if template.attach_shared_ebs {
        let ebs_vol = aws_client
            .create_volume()
            .availability_zone(template.instance_template.availability_zone)
            .size(template.shared_ebs_volume_size.unwrap().try_into()?)
            .volume_type(types::VolumeType::Gp3)
            .tag_specifications(
                types::TagSpecification::builder()
                    .resource_type(types::ResourceType::Volume)
                    .tags(
                        types::Tag::builder()
                            .key("project")
                            .value(template.instance_template.project_tag)
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await?;

        Some(ebs_vol.volume_id.unwrap())
    } else {
        None
    };

    // Create VPC
    // Create a VPC
    let vpc = aws_client
        .create_vpc()
        .cidr_block("10.0.0.0/24")
        .tag_specifications(
            types::TagSpecification::builder()
                .resource_type(types::ResourceType::Vpc)
                .tags(
                    types::Tag::builder()
                        .key("Name")
                        .value(template.cluster_name)
                        .build(),
                )
                .tags(
                    types::Tag::builder()
                        .key("project")
                        .value(template.project_tag)
                        .build(),
                )
                .build(),
        )
        .send()
        .await?
        .vpc
        .unwrap();

    // Create Subnet
    // TODO

    // Create Security Group
    // TODO

    // Create placement group
    // TODO

    // Loop to create instances
    // let mut futs = Vec::new();
    for i in 0..template.num_instances {
        // futs.push(create_instance_sdk(aws_client, &template.instance_template));

        // Create network interfaces
        // TODO

        // Create the instance with specific options based on the instance template
        // TODO
    }

    // Await futures
    // join_all(futs).await;

    // TODO

    Ok(())
}

/// Terminate a list of instances by their instance IDs.
///
/// # Arguments
/// * `aws_client` - The AWS client to use for the operation.
/// * `instance_ids` - A list of instance IDs to terminate.
///
/// # Returns
/// * A result indicating success or failure.
#[allow(unused)]
pub async fn terminate_instances(
    aws_client: &aws_sdk_ec2::Client,
    instance_ids: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terms = aws_client.terminate_instances();
    for id in instance_ids {
        terms = terms.instance_ids(id);
    }

    // Attempt to terminate the instances
    let _ = terms.send().await?;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct VpcCleanup {
    vpc_id: Option<String>,
    igw_id: Option<String>,
    subnet_ids: Option<Vec<String>>,
    route_table_ids: Option<Vec<String>>,
    security_group_ids: Option<Vec<String>>,
}

/// Print a message indicating that a clean-up operation is being performed.
///
/// Used pretty much like the `println!` macro. The arguments to the macro are provided to a `format!`
/// macro, and the resulting string is printed with a "CLEAN UP" prefix in yellow.
#[macro_export]
macro_rules! print_cln {
    ( $( $x:expr ),* ) => {
        println!("{}[CLEAN UP] {}{}", color::Fg(color::Yellow), format!($( $x, )*), style::Reset);
    };
}

#[allow(unused)]
pub async fn cleanup_vpc(
    aws_client: &aws_sdk_ec2::Client,
    cleanup_items: VpcCleanup,
) -> Result<(), Box<dyn std::error::Error>> {
    print_cln!("Received clean-up...");

    // Delete the security groups
    if let Some(security_group_ids) = cleanup_items.security_group_ids.clone() {
        for sg_id in security_group_ids {
            print_cln!("Deleting security group: {:#?}", sg_id);

            let del_sg_out = aws_client
                .delete_security_group()
                .group_id(sg_id)
                .send()
                .await?;
            print_cln!("Sent delete security group, got: {:#?}", del_sg_out);
        }
    } else {
        print_cln!("No security groups to delete.");
    }

    // Disassociate the IGW from the VPC
    if let Some(igw_id) = cleanup_items.igw_id.clone() {
        print_cln!("Disassociating IGW: {:#?}", igw_id);

        let disassoc_igw_out = aws_client
            .detach_internet_gateway()
            .internet_gateway_id(igw_id.clone())
            .vpc_id(cleanup_items.vpc_id.clone().unwrap())
            .send()
            .await?;
        print_cln!(
            "Sent disassociate internet gateway, got: {:#?}",
            disassoc_igw_out
        );
    } else {
        print_cln!("No IGW to disassociate.");
    }

    // Delete the IGW
    if let Some(igw_id) = cleanup_items.igw_id.clone() {
        print_cln!("Deleting IGW: {:#?}", igw_id);

        let del_igw_out = aws_client
            .delete_internet_gateway()
            .internet_gateway_id(igw_id)
            .send()
            .await?;
        print_cln!("Sent delete internet gateway, got: {:#?}", del_igw_out);
    } else {
        print_cln!("No IGW to delete.");
    }

    // Delete the subnets
    if let Some(subnet_ids) = cleanup_items.subnet_ids.clone() {
        for subnet_id in subnet_ids {
            print_cln!("Deleting subnet: {:#?}", subnet_id);

            let del_subnet_out = aws_client
                .delete_subnet()
                .subnet_id(subnet_id)
                .send()
                .await?;
            print_cln!("Sent delete subnet, got: {:#?}", del_subnet_out);
        }
    } else {
        print_cln!("No subnets to delete.");
    }

    // Disassociate all routes from route tables
    if let Some(route_table_ids) = cleanup_items.route_table_ids.clone() {
        for rt_id in route_table_ids {
            let routes = aws_client
                .describe_route_tables()
                .route_table_ids(rt_id.clone())
                .send()
                .await?
                .route_tables
                .unwrap();

            for rt in routes {
                let associations = rt.associations.unwrap();
                for assoc in associations {
                    if let Some(assoc_id) = assoc.route_table_association_id {
                        print_cln!("Disassociating route table: {:#?}", assoc_id);

                        // Try to disassociate the route table
                        let disassoc_out = match aws_client
                            .disassociate_route_table()
                            .association_id(assoc_id)
                            .send()
                            .await {
                            Ok(val) => val,
                            Err(e) => {
                                print_cln!(
                                    "[WARNING] Failed to disassociate route table: {:#?}",
                                    e
                                );

                                // Handle skipping if the association was not found
                                // Note: Not found means we can't disassociate it, so we just continue. There
                                //       may be a bug somewhere else that causes this though, so be careful.
                                match e.code() {
                                    Some(code) => {
                                        if code == "InvalidAssociationID.NotFound" {
                                            print_cln!("Because association not found, skipping disassociation. There might be a bug somewhere that caused this! Be careful!");

                                            // Just continue to next association
                                            continue;
                                        }

                                        return Err(Box::new(e));
                                    }
                                    None => {
                                        return Err(Box::new(e));
                                    }
                                };
                            },
                        };
                        print_cln!("Sent disassociate route table, got: {:#?}", disassoc_out);
                    }
                }
            }
        }
    } else {
        print_cln!("No route tables to disassociate.");
    }

    // Delete the route tables
    if let Some(route_table_ids) = cleanup_items.route_table_ids.clone() {
        for rt_id in route_table_ids {
            print_cln!("Deleting route table: {:#?}", rt_id);

            let del_rt_out = aws_client
                .delete_route_table()
                .route_table_id(rt_id)
                .send()
                .await?;
            print_cln!("Sent delete route table, got: {:#?}", del_rt_out);
        }
    } else {
        print_cln!("No route tables to delete.");
    }

    // Delete the VPC
    if let Some(vpc_id) = cleanup_items.vpc_id.clone() {
        print_cln!("Deleting VPC: {:#?}", vpc_id);

        let del_vpc_out = aws_client.delete_vpc().vpc_id(vpc_id).send().await?;
        print_cln!("Sent delete VPC, got: {:#?}", del_vpc_out);
    } else {
        print_cln!("No VPC to delete.");
    }

    print_cln!("Clean-up complete for VPC: {:#?}", cleanup_items.vpc_id);

    // Return success
    Ok(())
}

// #[tokio::test]
// pub async fn test_create_vpc() -> Result<(), Box<dyn std::error::Error>> {
//     // Set up AWS environment and client
//     let shared_config = aws_config::load_from_env().await;
//     let aws_client: aws_sdk_ec2::Client = aws_sdk_ec2::Client::new(&shared_config);

//     let mut vpc_cleanup_items = VpcCleanup {
//         vpc_id: None,
//         igw_id: None,
//         subnet_ids: None,
//         route_table_ids: None,
//         security_group_ids: None,
//     };

//     // Create a VPC
//     let vpc = aws_client
//         .create_vpc()
//         .cidr_block("10.0.0.0/16")
//         .amazon_provided_ipv6_cidr_block(true)
//         .ipv6_cidr_block_network_border_group("us-west-2")
//         .tag_specifications(
//             types::TagSpecification::builder()
//                 .resource_type(types::ResourceType::Vpc)
//                 .tags(
//                     types::Tag::builder()
//                         .key("Name")
//                         .value("Experimental Autocreated VPC")
//                         .build(),
//                 )
//                 .tags(
//                     types::Tag::builder()
//                         .key("project")
//                         .value("testing_sdk")
//                         .build(),
//                 )
//                 .build(),
//         )
//         .send()
//         .await?
//         .vpc
//         .unwrap();

//     // Print the VPC ID
//     let vpc_id = vpc.vpc_id.unwrap();
//     vpc_cleanup_items.vpc_id = Some(vpc_id.clone());
//     println!("[DEBUG] Created VPC with ID: {:#?}", vpc_id);

//     // Create a subnet for each availability zone
//     let azs = vec!["us-west-2a", "us-west-2b", "us-west-2c"];

//     let mut subnet_futures = Vec::new();
//     vpc_cleanup_items.subnet_ids = Some(Vec::new());
//     for (i, &az) in azs.iter().enumerate() {
//         // Use a different block for each subnet
//         let ipv4_cider_block = format!("10.0.{}.0/24", i);

//         // Create a subnet in the AZ
//         subnet_futures.push(
//             aws_client
//                 .create_subnet()
//                 .tag_specifications(
//                     types::TagSpecification::builder()
//                         .resource_type(types::ResourceType::Subnet)
//                         .tags(
//                             types::Tag::builder()
//                                 .key("Name")
//                                 .value(format!("Experimental Autocreated Subnet in {}", az))
//                                 .build(),
//                         )
//                         .tags(
//                             types::Tag::builder()
//                                 .key("project")
//                                 .value("testing_sdk")
//                                 .build(),
//                         )
//                         .build(),
//                 )
//                 .availability_zone(az)
//                 .cidr_block(ipv4_cider_block.clone())
//                 .vpc_id(vpc_id.clone())
//                 .send(),
//         );
//     }
//     println!("[DEBUG] Sent all subnet creation requests.");

//     // Wait for all subnets to be created
//     let subnet_results = futures::future::join_all(subnet_futures).await;
//     let mut subnets = Vec::new();

//     for result in subnet_results.iter() {
//         match result {
//             Ok(create_subnet_output) => {
//                 println!("Subnet creation request: {:#?}", create_subnet_output);
//                 match create_subnet_output.subnet.clone() {
//                     Some(subnet) => {
//                         subnets.push(subnet);
//                     }
//                     None => {
//                         println!("[ERROR] No subnet was returned in the response!");

//                         // Clean up
//                         cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                         panic!("[ERROR] No subnet was returned in the response!");
//                     }
//                 }
//             }
//             Err(e) => {
//                 println!("[ERROR] Subnet creation failed: {:#?}", e);

//                 // Clean up
//                 cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                 panic!("[ERROR] Subnet creation failed: {:#?}", e);
//             }
//         };
//     }
//     println!("Created subnets: {:#?}", subnets);
//     vpc_cleanup_items.subnet_ids = Some(
//         subnets
//             .iter()
//             .map(|s| s.subnet_id.clone().unwrap())
//             .collect(),
//     );
//     println!(
//         "[DEBUG] Created subnets: {:#?}",
//         subnets
//             .iter()
//             .map(|s| s.subnet_id.clone().unwrap())
//             .collect::<Vec<String>>()
//     );

//     // Create an internet gateway
//     // Note: This is necessary to allow instances to communicate with the internet
//     let igw = {
//         let result = aws_client
//             .create_internet_gateway()
//             .tag_specifications(
//                 types::TagSpecification::builder()
//                     .resource_type(types::ResourceType::InternetGateway)
//                     .tags(
//                         types::Tag::builder()
//                             .key("Name")
//                             .value("Experimental Autocreated IGW")
//                             .build(),
//                     )
//                     .tags(
//                         types::Tag::builder()
//                             .key("project")
//                             .value("testing_sdk")
//                             .build(),
//                     )
//                     .build(),
//             )
//             .send()
//             .await;

//         match result {
//             Ok(val) => val.internet_gateway.unwrap(),
//             Err(e) => {
//                 println!("[ERROR] Failed to create internet gateway: {:#?}", e);

//                 // Clean up
//                 cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                 panic!("[ERROR] Failed to create internet gateway: {:#?}", e);
//             }
//         }
//     };
//     let igw_id = igw.internet_gateway_id.as_ref().unwrap();
//     vpc_cleanup_items.igw_id = Some(igw_id.clone());
//     println!("[DEBUG] Created internet gateway: {:#?}", igw);

//     // Attach the internet gateway to the VPC
//     let attach_igw_output = match aws_client
//         .attach_internet_gateway()
//         .internet_gateway_id(igw_id.clone())
//         .vpc_id(vpc_id.clone())
//         .send()
//         .await
//     {
//         Ok(val) => val,
//         Err(e) => {
//             println!("[ERROR] Failed to attach internet gateway to VPC: {:#?}", e);

//             // Clean up
//             cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//             panic!("[ERROR] Failed to attach internet gateway to VPC: {:#?}", e);
//         }
//     };
//     println!("Attached internet gateway to VPC: {:#?}", attach_igw_output);

//     // Create a route table
//     let route_table = {
//         let result = aws_client
//             .create_route_table()
//             .tag_specifications(
//                 types::TagSpecification::builder()
//                     .resource_type(types::ResourceType::RouteTable)
//                     .tags(
//                         types::Tag::builder()
//                             .key("Name")
//                             .value("Experimental Autocreated Route Table")
//                             .build(),
//                     )
//                     .tags(
//                         types::Tag::builder()
//                             .key("project")
//                             .value("testing_sdk")
//                             .build(),
//                     )
//                     .build(),
//             )
//             .vpc_id(vpc_id.clone())
//             .send()
//             .await;

//         match result {
//             Ok(val) => val.route_table.unwrap(),
//             Err(e) => {
//                 println!("[ERROR] Failed to create route table: {:#?}", e);

//                 // Clean up
//                 cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                 panic!("[ERROR] Failed to create route table: {:#?}", e);
//             }
//         }
//     };
//     let route_table_id = route_table.route_table_id.as_ref().unwrap().clone();
//     vpc_cleanup_items.route_table_ids = Some(vec![route_table_id.clone()]);
//     println!("[DEBUG] Created route table: {:#?}", route_table_id);

//     // Add routes to the route table
//     {
//         let mut futures = Vec::new();

//         // Submit 'add route' requests

//         futures.push(
//             aws_client
//                 .create_route()
//                 .destination_cidr_block("0.0.0.0/0")
//                 .gateway_id(igw_id.clone())
//                 .route_table_id(route_table_id.clone())
//                 .send(),
//         );

//         futures.push(
//             aws_client
//                 .create_route()
//                 .destination_ipv6_cidr_block("::/0")
//                 .gateway_id(igw_id.clone())
//                 .route_table_id(route_table_id.clone())
//                 .send(),
//         );

//         // Wait for all routes to be created
//         let results = futures::future::join_all(futures).await;

//         // Handle the results
//         for result in results {
//             match result {
//                 Ok(val) => {
//                     println!("Created route: {:#?}", val);
//                 }
//                 Err(e) => {
//                     println!("[ERROR] Failed to create route: {:#?}", e);

//                     // Clean up
//                     cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                     panic!("[ERROR] Failed to create route: {:#?}", e);
//                 }
//             }
//         }
//     };

//     // Associate the route table with each subnet
//     for subnet in &subnets {
//         let subnet_id = subnet.subnet_id().unwrap();

//         let assoc = match aws_client
//             .associate_route_table()
//             .route_table_id(route_table.route_table_id.as_ref().unwrap())
//             .subnet_id(subnet_id)
//             .send()
//             .await
//         {
//             Ok(val) => val,
//             Err(e) => {
//                 println!(
//                     "[ERROR] Failed to associate route table with subnet: {:#?}",
//                     e
//                 );

//                 // Clean up
//                 cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                 panic!(
//                     "[ERROR] Failed to associate route table with subnet: {:#?}",
//                     e
//                 );
//             }
//         };

//         // Verify correct association state
//         // Note: This is to ensure that the route table is actually associated with the subnet
//         match assoc.association_state() {
//             Some(state) => {
//                 println!("Associated route table with subnet: {:#?}", state);
//             }
//             None => {
//                 println!("[WARNING] No association state was returned in the response!");

//                 // Clean up
//                 cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                 panic!("[ERROR] No association state was returned in the response!");
//             }
//         }

//         println!("Associated route table with subnet: {:#?}", assoc);
//     }

//     // Create security group
//     let sg_name = "experimental-sdk-sg";
//     let create_sg_output = match aws_client
//         .create_security_group()
//         .description("Experimental Autocreated Security Group")
//         .group_name(sg_name)
//         .tag_specifications(
//             types::TagSpecification::builder()
//                 .resource_type(types::ResourceType::SecurityGroup)
//                 .tags(
//                     types::Tag::builder()
//                         .key("Name")
//                         .value("Experimental Autocreated Security Group")
//                         .build(),
//                 )
//                 .tags(
//                     types::Tag::builder()
//                         .key("project")
//                         .value("testing_sdk")
//                         .build(),
//                 )
//                 .build(),
//         )
//         .vpc_id(vpc_id.clone())
//         .send()
//         .await
//     {
//         Ok(val) => val,
//         Err(e) => {
//             println!("[ERROR] Failed to create security group: {:#?}", e);

//             // Clean up
//             cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//             panic!("[ERROR] Failed to create security group: {:#?}", e);
//         }
//     };
//     println!("Created security group: {:#?}", create_sg_output);
//     let sg_id = create_sg_output.group_id.unwrap();
//     vpc_cleanup_items.security_group_ids = Some(vec![sg_id.clone()]);
//     println!("[DEBUG] Created security group: {:#?}", sg_id);

//     // Add ingress/egress rules to the security group
//     let sec_group_ingress_out = {
//         let ip_perm_ssh = types::IpPermission::builder()
//             .from_port(22)
//             .to_port(22)
//             .ip_protocol("tcp")
//             .ip_ranges(
//                 types::IpRange::builder()
//                     .cidr_ip("0.0.0.0/0")
//                     .description("Allow SSH from anywhere (ipv4)")
//                     .build(),
//             )
//             .ipv6_ranges(
//                 types::Ipv6Range::builder()
//                     .cidr_ipv6("::/0")
//                     .description("Allow SSH from anywhere (ipv6)")
//                     .build(),
//             )
//             .build();

//         // Not needed for now
//         // let ip_perm_http = types::IpPermission::builder()
//         //     .from_port(80)
//         //     .to_port(80)
//         //     .ip_protocol("tcp")
//         //     .ip_ranges(types::IpRange::builder()
//         //         .cidr_ip("0.0.0.0/0")
//         //         .description("Allow HTTP from anywhere (ipv4)")
//         //         .build())
//         //     .ipv6_ranges(types::Ipv6Range::builder()
//         //             .cidr_ipv6("::/0")
//         //             .description("Allow HTTP from anywhere (ipv6)")
//         //             .build())
//         //     .build();

//         // Add SSH ingress rule
//         match aws_client
//             .authorize_security_group_ingress()
//             .group_id(sg_id.clone())
//             .ip_permissions(ip_perm_ssh)
//             // .ip_permissions(ip_perm_http)
//             // .source_security_group_name(sg_id.clone())
//             .send()
//             .await
//         {
//             Ok(val) => val,
//             Err(e) => {
//                 println!(
//                     "[ERROR] Failed to add SSH ingress rules to security group: {:#?}",
//                     e
//                 );

//                 // Clean up
//                 cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//                 panic!(
//                     "[ERROR] Failed to add SSH ingress rules to security group: {:#?}",
//                     e
//                 );
//             }
//         }
//     };
//     println!(
//         "Added ingress rules to security group: {:#?}",
//         sec_group_ingress_out
//     );

//     println!(
//         "🎉🎉🎉 {}Finished setting up the entire VPC! (ID: {:#?}){} 🎉🎉🎉",
//         color::Fg(color::Green),
//         vpc_id,
//         style::Reset
//     );
//     println!("Now waiting for a few seconds before destroying...");

//     // Wait a few seconds
//     tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;

//     // Run cleanup
//     cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

//     // Return success
//     Ok(())
// }

/// Create a VPC.
/// 
/// # Returns
/// * The ID of the created VPC as a `String` and a `VpcCleanup` struct that can be used to nuke the VPC, or errors.
pub async fn create_vpc(aws_client: &Client, vpc_name: &str, project_tag: &str) -> Result<(String, VpcCleanup), Box<dyn std::error::Error>> {

    // Create the struct that can be used to nuke the VPC
    let mut vpc_cleanup_items = VpcCleanup {
        vpc_id: None,
        igw_id: None,
        subnet_ids: None,
        route_table_ids: None,
        security_group_ids: None,
    };

    // Create a VPC
    let vpc = aws_client
        .create_vpc()
        .cidr_block("10.0.0.0/16")
        .amazon_provided_ipv6_cidr_block(true)
        .ipv6_cidr_block_network_border_group("us-west-2")
        .tag_specifications(
            types::TagSpecification::builder()
                .resource_type(types::ResourceType::Vpc)
                .tags(
                    types::Tag::builder()
                        .key("Name")
                        .value(vpc_name)
                        .build(),
                )
                .tags(
                    types::Tag::builder()
                        .key("project")
                        .value(project_tag)
                        .build(),
                )
                .build(),
        )
        .send()
        .await?
        .vpc
        .unwrap();

    // Print the VPC ID
    let vpc_id = vpc.vpc_id.unwrap();
    vpc_cleanup_items.vpc_id = Some(vpc_id.clone());
    println!("[DEBUG] Created VPC with ID: {:#?}", vpc_id);

    // Create a subnet for each availability zone
    let azs = vec!["us-west-2a", "us-west-2b", "us-west-2c"];

    let mut subnet_futures = Vec::new();
    vpc_cleanup_items.subnet_ids = Some(Vec::new());
    for (i, &az) in azs.iter().enumerate() {
        // Use a different block for each subnet
        let ipv4_cider_block = format!("10.0.{}.0/24", i);

        // Create a subnet in the AZ
        subnet_futures.push(
            aws_client
                .create_subnet()
                .tag_specifications(
                    types::TagSpecification::builder()
                        .resource_type(types::ResourceType::Subnet)
                        .tags(
                            types::Tag::builder()
                                .key("Name")
                                .value(format!("Autocreated Subnet for {} in {}", vpc_name, az))
                                .build(),
                        )
                        .tags(
                            types::Tag::builder()
                                .key("project")
                                .value(project_tag)
                                .build(),
                        )
                        .build(),
                )
                .availability_zone(az)
                .cidr_block(ipv4_cider_block.clone())
                .vpc_id(vpc_id.clone())
                .send(),
        );
    }
    println!("[DEBUG] Sent all subnet creation requests.");

    // Wait for all subnets to be created
    let subnet_results = futures::future::join_all(subnet_futures).await;
    let mut subnets = Vec::new();

    for result in subnet_results.iter() {
        match result {
            Ok(create_subnet_output) => {
                println!("Subnet creation request: {:#?}", create_subnet_output);
                match create_subnet_output.subnet.clone() {
                    Some(subnet) => {
                        subnets.push(subnet);
                    }
                    None => {
                        println!("[ERROR] No subnet was returned in the response!");

                        // Clean up
                        cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                        panic!("[ERROR] No subnet was returned in the response!");
                    }
                }
            }
            Err(e) => {
                println!("[ERROR] Subnet creation failed: {:#?}", e);

                // Clean up
                cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                panic!("[ERROR] Subnet creation failed: {:#?}", e);
            }
        };
    }
    println!("Created subnets: {:#?}", subnets);
    vpc_cleanup_items.subnet_ids = Some(
        subnets
            .iter()
            .map(|s| s.subnet_id.clone().unwrap())
            .collect(),
    );
    println!(
        "[DEBUG] Created subnets: {:#?}",
        subnets
            .iter()
            .map(|s| s.subnet_id.clone().unwrap())
            .collect::<Vec<String>>()
    );

    // Create an internet gateway
    // Note: This is necessary to allow instances to communicate with the internet
    let igw = {
        let result = aws_client
            .create_internet_gateway()
            .tag_specifications(
                types::TagSpecification::builder()
                    .resource_type(types::ResourceType::InternetGateway)
                    .tags(
                        types::Tag::builder()
                            .key("Name")
                            .value(format!("Autocreated IGW for {}", vpc_name))
                            .build(),
                    )
                    .tags(
                        types::Tag::builder()
                            .key("project")
                            .value(project_tag)
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(val) => val.internet_gateway.unwrap(),
            Err(e) => {
                println!("[ERROR] Failed to create internet gateway: {:#?}", e);

                // Clean up
                cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                panic!("[ERROR] Failed to create internet gateway: {:#?}", e);
            }
        }
    };
    let igw_id = igw.internet_gateway_id.as_ref().unwrap();
    vpc_cleanup_items.igw_id = Some(igw_id.clone());
    println!("[DEBUG] Created internet gateway: {:#?}", igw);

    // Attach the internet gateway to the VPC
    let attach_igw_output = match aws_client
        .attach_internet_gateway()
        .internet_gateway_id(igw_id.clone())
        .vpc_id(vpc_id.clone())
        .send()
        .await
    {
        Ok(val) => val,
        Err(e) => {
            println!("[ERROR] Failed to attach internet gateway to VPC: {:#?}", e);

            // Clean up
            cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

            panic!("[ERROR] Failed to attach internet gateway to VPC: {:#?}", e);
        }
    };
    println!("Attached internet gateway to VPC: {:#?}", attach_igw_output);

    // Create a route table
    let route_table = {
        let result = aws_client
            .create_route_table()
            .tag_specifications(
                types::TagSpecification::builder()
                    .resource_type(types::ResourceType::RouteTable)
                    .tags(
                        types::Tag::builder()
                            .key("Name")
                            .value(format!("Autocreated Route Table for {}", vpc_name))
                            .build(),
                    )
                    .tags(
                        types::Tag::builder()
                            .key("project")
                            .value(project_tag)
                            .build(),
                    )
                    .build(),
            )
            .vpc_id(vpc_id.clone())
            .send()
            .await;

        match result {
            Ok(val) => val.route_table.unwrap(),
            Err(e) => {
                println!("[ERROR] Failed to create route table: {:#?}", e);

                // Clean up
                cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                panic!("[ERROR] Failed to create route table: {:#?}", e);
            }
        }
    };
    let route_table_id = route_table.route_table_id.as_ref().unwrap().clone();
    vpc_cleanup_items.route_table_ids = Some(vec![route_table_id.clone()]);
    println!("[DEBUG] Created route table: {:#?}", route_table_id);

    // Add routes to the route table
    {
        let mut futures = Vec::new();

        // Submit 'add route' requests

        futures.push(
            aws_client
                .create_route()
                .destination_cidr_block("0.0.0.0/0")
                .gateway_id(igw_id.clone())
                .route_table_id(route_table_id.clone())
                .send(),
        );

        futures.push(
            aws_client
                .create_route()
                .destination_ipv6_cidr_block("::/0")
                .gateway_id(igw_id.clone())
                .route_table_id(route_table_id.clone())
                .send(),
        );

        // Wait for all routes to be created
        let results = futures::future::join_all(futures).await;

        // Handle the results
        for result in results {
            match result {
                Ok(val) => {
                    println!("Created route: {:#?}", val);
                }
                Err(e) => {
                    println!("[ERROR] Failed to create route: {:#?}", e);

                    // Clean up
                    cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                    panic!("[ERROR] Failed to create route: {:#?}", e);
                }
            }
        }
    };

    // Associate the route table with each subnet
    for subnet in &subnets {
        let subnet_id = subnet.subnet_id().unwrap();

        let assoc = match aws_client
            .associate_route_table()
            .route_table_id(route_table.route_table_id.as_ref().unwrap())
            .subnet_id(subnet_id)
            .send()
            .await
        {
            Ok(val) => val,
            Err(e) => {
                println!(
                    "[ERROR] Failed to associate route table with subnet: {:#?}",
                    e
                );

                // Clean up
                cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                panic!(
                    "[ERROR] Failed to associate route table with subnet: {:#?}",
                    e
                );
            }
        };

        // Verify correct association state
        // Note: This is to ensure that the route table is actually associated with the subnet
        match assoc.association_state() {
            Some(state) => {
                println!("Associated route table with subnet: {:#?}", state);
            }
            None => {
                println!("[WARNING] No association state was returned in the response!");

                // Clean up
                cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                panic!("[ERROR] No association state was returned in the response!");
            }
        }

        println!("Associated route table with subnet: {:#?}", assoc);
    }

    // Create security group
    let sg_name = "experimental-sdk-sg";
    let create_sg_output = match aws_client
        .create_security_group()
        .description("Experimental Autocreated Security Group")
        .group_name(sg_name)
        .tag_specifications(
            types::TagSpecification::builder()
                .resource_type(types::ResourceType::SecurityGroup)
                .tags(
                    types::Tag::builder()
                        .key("Name")
                        .value(format!("Autocreated Security Group for {}", vpc_name))
                        .build(),
                )
                .tags(
                    types::Tag::builder()
                        .key("project")
                        .value(project_tag)
                        .build(),
                )
                .build(),
        )
        .vpc_id(vpc_id.clone())
        .send()
        .await
    {
        Ok(val) => val,
        Err(e) => {
            println!("[ERROR] Failed to create security group: {:#?}", e);

            // Clean up
            cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

            panic!("[ERROR] Failed to create security group: {:#?}", e);
        }
    };
    println!("Created security group: {:#?}", create_sg_output);
    let sg_id = create_sg_output.group_id.unwrap();
    vpc_cleanup_items.security_group_ids = Some(vec![sg_id.clone()]);
    println!("[DEBUG] Created security group: {:#?}", sg_id);

    // Add ingress/egress rules to the security group
    let sec_group_ingress_out = {
        let ip_perm_ssh = types::IpPermission::builder()
            .from_port(22)
            .to_port(22)
            .ip_protocol("tcp")
            .ip_ranges(
                types::IpRange::builder()
                    .cidr_ip("0.0.0.0/0")
                    .description("Allow SSH from anywhere (ipv4)")
                    .build(),
            )
            .ipv6_ranges(
                types::Ipv6Range::builder()
                    .cidr_ipv6("::/0")
                    .description("Allow SSH from anywhere (ipv6)")
                    .build(),
            )
            .build();

        // Not needed for now
        // let ip_perm_http = types::IpPermission::builder()
        //     .from_port(80)
        //     .to_port(80)
        //     .ip_protocol("tcp")
        //     .ip_ranges(types::IpRange::builder()
        //         .cidr_ip("0.0.0.0/0")
        //         .description("Allow HTTP from anywhere (ipv4)")
        //         .build())
        //     .ipv6_ranges(types::Ipv6Range::builder()
        //             .cidr_ipv6("::/0")
        //             .description("Allow HTTP from anywhere (ipv6)")
        //             .build())
        //     .build();

        // Add intra-security group allow all rule
        let intra_sg_all = types::IpPermission::builder()
            .from_port(0)
            .to_port(65535)
            .ip_protocol("-1")
            .user_id_group_pairs(
                types::UserIdGroupPair::builder()
                    .group_id(sg_id.clone())
                    .description("Allow all traffic within the security group")
                    .build(),
            )
            .build();

        // Add SSH ingress rule
        match aws_client
            .authorize_security_group_ingress()
            .group_id(sg_id.clone())
            .ip_permissions(ip_perm_ssh)
            .ip_permissions(intra_sg_all)
            // .ip_permissions(ip_perm_http)
            // .source_security_group_name(sg_id.clone())
            .send()
            .await
        {
            Ok(val) => val,
            Err(e) => {
                println!(
                    "[ERROR] Failed to add SSH ingress rules to security group: {:#?}",
                    e
                );

                // Clean up
                cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;

                panic!(
                    "[ERROR] Failed to add SSH ingress rules to security group: {:#?}",
                    e
                );
            }
        }
    };
    println!(
        "Added ingress rules to security group: {:#?}",
        sec_group_ingress_out
    );

    println!(
        "🎉🎉🎉 {}Finished setting up the entire VPC! (ID: {:#?}){} 🎉🎉🎉",
        color::Fg(color::Green),
        vpc_id,
        style::Reset
    );

    println!("{}[IMPORTANT] 💣💣💣 Here is the info you'll need to nuke the VPC 💣💣💣: {:#?}{}", color::Fg(color::Magenta), vpc_cleanup_items, style::Reset);

    // Return the VPC ID
    Ok((vpc_id, vpc_cleanup_items))
}

#[tokio::test]
pub async fn test_create_vpc() -> Result<(), Box<dyn std::error::Error>> {
    // Set up AWS environment and client
    let shared_config = aws_config::load_from_env().await;
    let aws_client: aws_sdk_ec2::Client = aws_sdk_ec2::Client::new(&shared_config);

    println!("{}[TEST_CREATE_VPC] Attempting to create a VPC...{}", color::Fg(color::Yellow), style::Reset);

    // Create the VPC
    let (vpc_id, vpc_cleanup_items) = create_vpc(&aws_client, "Experimental Autocreated VPC", "testing_sdk").await?;

    let time_delay = 15;
    println!("{}[TEST_CREATE_VPC] Success! Created VPC with ID: {:#?}{} Waiting {} second(s) before destroying...", color::Fg(color::Green), vpc_id, style::Reset, time_delay);

    // Wait a few seconds
    tokio::time::sleep(tokio::time::Duration::from_secs(time_delay)).await;

    // Run cleanup
    println!("{}[TEST_CREATE_VPC] Attempting to clean up VPC...{}", color::Fg(color::Yellow), style::Reset);
    cleanup_vpc(&aws_client, vpc_cleanup_items.clone()).await?;
    println!("{}[TEST_CREATE_VPC] Success! Cleaned up VPC with ID: {:#?}{}", color::Fg(color::Green), vpc_id, style::Reset);

    // Return success
    Ok(())
}
