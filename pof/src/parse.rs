use core::panic;
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::{self};
use std::io::{ErrorKind, Read, Seek, SeekFrom};

use crate::*;
use byteorder::{ReadBytesExt, LE};
use dae_parser::source::{SourceReader, ST, XYZ};
use dae_parser::{Document, Material};

impl<'a> dae_parser::geom::VertexLoad<'a, (u16, u16)> for PolyVertex {
    fn position(ctx: &(u16, u16), _: &SourceReader<'a, XYZ>, index: u32) -> Self {
        PolyVertex {
            vertex_id: VertexId(u16::try_from(index).unwrap() + ctx.0),
            normal_id: NormalId(0),
            uv: (0.0, 0.0),
        }
    }
    fn add_normal(&mut self, ctx: &(u16, u16), _: &SourceReader<'a, XYZ>, index: u32) {
        self.normal_id = NormalId(u16::try_from(index).unwrap() + ctx.1);
    }
    fn add_texcoord(&mut self, _: &(u16, u16), reader: &SourceReader<'a, ST>, index: u32, set: Option<u32>) {
        assert!(set.map_or(true, |set| set == 0));
        let [u, v] = reader.get(index as usize);
        self.uv = (u, v);
    }
}

pub fn parse_dae(path: impl AsRef<std::path::Path>) -> Box<Model> {
    let document = Document::from_file(path).unwrap();
    // use std::io::Write;
    // write!(std::fs::File::create("output.log").unwrap(), "{:#?}", document).unwrap();
    let mut sub_objects = vec![];
    let map = document.local_maps();
    let scene = map
        .get(&document.scene.as_ref().unwrap().instance_visual_scene.as_ref().unwrap().url)
        .unwrap();

    let mut materials = vec![];
    let mut material_map = HashMap::new();
    document.for_each(|material: &Material| {
        material_map.insert(material.id.as_ref().unwrap(), TextureId(materials.len() as u32));
        materials.push(material.name.as_ref().unwrap().clone());
    });
    let mut details = vec![];
    let mut shield_data = None;

    fn flip_y_z(vec: Vec3d) -> Vec3d {
        Vec3d { x: vec.x, y: vec.z, z: vec.y }
    }

    for node in &scene.nodes {
        let transform = node.transform_as_matrix();
        let zero = Vec3d::ZERO.into();
        let center = transform.transform_point(&zero) - zero;
        let local_transform = transform.append_translation(&(-center).into());
        // println!("{:#?}", center);
        // println!("{:#?}", local_transform);
        let mut vertices_out: Vec<Vec3d> = vec![];
        let mut normals_out: Vec<Vec3d> = vec![];
        let mut offsets = (vertices_out.len() as u16, normals_out.len() as u16);
        let mut polygons_out = vec![];

        for geo in &node.instance_geometry {
            let geo = map[&geo.url].element.as_mesh().unwrap();
            let verts = geo.vertices.as_ref().unwrap().importer(&map).unwrap();
            offsets.0 = vertices_out.len() as u16;
            let mut iter = Clone::clone(verts.position_importer().unwrap());
            while let Some(position) = iter.next() {
                vertices_out.push(flip_y_z(&local_transform * Vec3d::from(position)));
            }

            for prim_elem in &geo.elements {
                match prim_elem {
                    dae_parser::Primitive::PolyList(polies) => {
                        let texture = match &polies.material {
                            Some(mat) => Texturing::Texture(material_map[mat]),
                            None => Texturing::Flat(Color::default()),
                        };

                        let importer = polies.importer(&map, verts.clone()).unwrap();

                        offsets.1 = normals_out.len() as u16;
                        for normal in Clone::clone(importer.normal_importer().unwrap()) {
                            normals_out.push(flip_y_z(&local_transform * Vec3d::from(normal)));
                        }

                        let mut iter = importer.read::<_, PolyVertex>(&offsets, &polies.data.prim);

                        for &n in &*polies.data.vcount {
                            let verts = (0..n).map(|_| iter.next().unwrap()).collect();
                            polygons_out.push((texture, verts));
                        }
                    }
                    dae_parser::Primitive::Triangles(tris) => {
                        let texture = match &tris.material {
                            Some(mat) => Texturing::Texture(material_map[mat]),
                            None => Texturing::Flat(Color::default()),
                        };
                        let importer = tris.importer(&map, verts.clone()).unwrap();

                        offsets.1 = normals_out.len() as u16;
                        for normal in Clone::clone(importer.normal_importer().expect("normals missing in DAE")) {
                            normals_out.push(flip_y_z(&local_transform * Vec3d::from(normal)));
                        }

                        let mut iter = importer.read::<_, PolyVertex>(&offsets, tris.data.prim.as_ref().unwrap());
                        while let Some(vert1) = iter.next() {
                            polygons_out.push((texture, vec![vert1, iter.next().unwrap(), iter.next().unwrap()]));
                        }
                    }
                    _ => {}
                }
            }
        }

        for poly in &mut polygons_out {
            poly.1.reverse();
        }

        let obj_id = ObjectId(sub_objects.len() as _);
        let name = node.name.as_ref().unwrap();
        if name == "shield" {
            let mut polygons = vec![];
            for (_, verts) in polygons_out {
                let verts = verts.into_iter().map(|poly| poly.vertex_id).collect::<Vec<_>>();
                if let [vert1, ref rest @ ..] = *verts {
                    for slice in rest.windows(2) {
                        if let [vert2, vert3] = *slice {
                            let [v1, v2, v3] = [vert1, vert2, vert3].map(|i| nalgebra_glm::Vec3::from(vertices_out[i.0 as usize]));
                            polygons.push(ShieldPolygon {
                                normal: (v2 - v1).cross(&(v3 - v1)).normalize().into(),
                                verts: (vert1, vert2, vert3),
                                neighbors: Default::default(),
                            })
                        }
                    }
                }
            }
            shield_data = dbg!(Some(ShieldData { verts: vertices_out, polygons, collision_tree: None }));
        } else {
            if name.starts_with("detail") {
                details.push((&**name, obj_id));
            }
            sub_objects.push(SubObject {
                obj_id,
                radius: Default::default(),
                parent: Default::default(),
                offset: flip_y_z(center.into()),
                geo_center: Default::default(),
                bbox: Default::default(),
                name: name.clone(),
                properties: Default::default(),
                movement_type: Default::default(),
                movement_axis: Default::default(),
                bsp_data: BspData {
                    verts: vertices_out,
                    norms: normals_out,
                    collision_tree: BspNode::Leaf {
                        bbox: Default::default(),
                        poly_list: polygons_out
                            .into_iter()
                            .map(|(texture, verts)| Polygon {
                                normal: Default::default(),
                                center: Default::default(),
                                radius: Default::default(),
                                texture,
                                verts,
                            })
                            .collect(),
                    },
                },
                children: Default::default(),
                is_debris_model: name.starts_with("debris"),
            })
        }
    }

    details.sort_by_key(|pair| pair.0);

    Box::new(Model {
        header: ObjHeader {
            max_radius: 1.0,
            num_subobjects: sub_objects.len() as _,
            detail_levels: details.into_iter().map(|pair| pair.1).collect(),
            ..Default::default()
        },
        sub_objects: ObjVec(sub_objects),
        textures: materials,
        paths: Default::default(),
        special_points: Default::default(),
        eye_points: Default::default(),
        primary_weps: Default::default(),
        secondary_weps: Default::default(),
        turrets: Default::default(),
        thruster_banks: Default::default(),
        glow_banks: Default::default(),
        auto_center: Default::default(),
        comments: Default::default(),
        docking_bays: Default::default(),
        insignias: Default::default(),
        shield_data,
    })
}

pub struct Parser<R> {
    file: R,
    version: Version,
}
impl<R: Read + Seek> Parser<R> {
    pub fn new(mut file: R) -> io::Result<Parser<R>> {
        assert!(&read_bytes(&mut file)? == b"PSPO", "Not a freespace 2 pof file!");

        let version: Version = read_i32(&mut file)?.try_into().expect("nani kono baasion");

        //println!("The verison is {:?}", version);

        Ok(Parser { file, version })
    }

    pub fn parse(&mut self) -> io::Result<Model> {
        // println!("parsing new model!");
        let mut header = None;
        let mut sub_objects = ObjVec::default();
        let mut textures = None;
        let mut paths = None;
        let mut special_points = None;
        let mut eye_points = None;
        let mut primary_weps = None;
        let mut secondary_weps = None;
        let mut turrets = vec![];
        let mut thruster_banks = None;
        let mut comments = None;
        let mut dock_points = None;
        let mut glow_banks = None;
        let mut insignias = None;
        let mut auto_center = None;
        let mut shield_data = None;

        let mut shield_tree_chunk = None;
        let mut debris_objs = vec![];

        loop {
            let id = &match self.read_bytes() {
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
                id_result => id_result?,
            };
            let len = self.read_i32()?;

            // println!("found chunk {}", std::str::from_utf8(id).unwrap());
            // println!("length is {} bytes", len);
            match id {
                b"HDR2" => {
                    assert!(header.is_none());

                    let max_radius = self.read_f32()?;
                    let obj_flags = self.read_u32()?;
                    let num_subobjects = self.read_u32()?;
                    let bounding_box = self.read_bbox()?;

                    let detail_levels = self.read_list(|this| Ok(ObjectId(this.read_u32()?)))?;
                    debris_objs = self.read_list(|this| Ok(ObjectId(this.read_u32()?)))?;

                    // todo worry about verioning
                    let mass = self.read_f32()?;
                    let center_of_mass = self.read_vec3d()?;
                    let moment_of_inertia = Mat3d {
                        rvec: self.read_vec3d()?,
                        uvec: self.read_vec3d()?,
                        fvec: self.read_vec3d()?,
                    };

                    let num_cross_sections = match self.read_u32()? {
                        u32::MAX => 0,
                        n => n,
                    };
                    let cross_sections = self.read_list_n(num_cross_sections as usize, |this| Ok((this.read_f32()?, this.read_f32()?)))?;

                    let bsp_lights = self.read_list(|this| {
                        Ok(BspLight {
                            location: this.read_vec3d()?,
                            kind: match this.read_u32()? {
                                1 => BspLightKind::Muzzle,
                                2 => BspLightKind::Thruster,
                                _ => panic!(), // maybe dont just panic
                            },
                        })
                    })?;

                    header = Some(ObjHeader {
                        num_subobjects,
                        max_radius,
                        obj_flags,
                        bounding_box,
                        detail_levels,
                        mass,
                        center_of_mass,
                        moment_of_inertia,
                        cross_sections,
                        bsp_lights,
                    });
                    //println!("{:#?}", header)
                }
                b"OBJ2" => {
                    let obj_id = ObjectId(self.read_u32().unwrap()); //id

                    let radius = self.read_f32().unwrap();
                    let parent = match self.read_u32().unwrap() {
                        u32::MAX => None,
                        parent_id => Some(ObjectId(parent_id)),
                    };
                    let offset = self.read_vec3d().unwrap();

                    let geo_center = self.read_vec3d().unwrap();
                    let bbox = self.read_bbox().unwrap();
                    let name = self.read_string().unwrap();
                    let properties = self.read_string().unwrap();
                    let movement_type = match self.read_i32().unwrap() {
                        -1 => SubsysMovementType::NONE,
                        0 => SubsysMovementType::POS,
                        1 => SubsysMovementType::ROT,
                        2 => SubsysMovementType::ROTSPECIAL,
                        3 => SubsysMovementType::TRIGGERED,
                        4 => SubsysMovementType::INTRINSICROTATE,
                        _ => unreachable!(),
                    };
                    let movement_axis = match self.read_i32().unwrap() {
                        -1 => SubsysMovementAxis::NONE,
                        0 => SubsysMovementAxis::XAXIS,
                        1 => SubsysMovementAxis::ZAXIS,
                        2 => SubsysMovementAxis::YAXIS,
                        3 => SubsysMovementAxis::OTHER,
                        _ => unreachable!(),
                    };

                    let _ = self.read_i32().unwrap();
                    let bsp_data_buffer = self.read_byte_buffer().unwrap();
                    let bsp_data = parse_bsp_data(&bsp_data_buffer);
                    //println!("parsed subobject {}", name);

                    sub_objects.push(SubObject {
                        obj_id,
                        radius,
                        parent,
                        offset,
                        geo_center,
                        bbox,
                        name,
                        properties,
                        movement_type,
                        movement_axis,
                        bsp_data,
                        // these two are to be filled later once we've parsed all the subobjects
                        children: vec![],
                        is_debris_model: false,
                    });
                    //println!("parsed subobject {:#?}", sub_objects.last());
                }
                b"TXTR" => {
                    assert!(textures.is_none());

                    textures = Some(self.read_list(|this| this.read_string())?);
                    //println!("{:#?}", textures);
                }
                b"PATH" => {
                    assert!(paths.is_none());

                    paths = Some(self.read_list(|this| {
                        Ok(Path {
                            name: this.read_string()?,
                            parent: this.read_string()?,
                            points: this.read_list(|this| {
                                Ok(PathPoint {
                                    position: this.read_vec3d()?,
                                    radius: this.read_f32()?,
                                    turrets: this.read_list(|this| Ok(ObjectId(this.read_u32()?)))?,
                                })
                            })?,
                        })
                    })?);
                    //println!("{:#?}", paths);
                }
                b"SPCL" => {
                    assert!(special_points.is_none());

                    special_points = Some(self.read_list(|this| {
                        Ok(SpecialPoint {
                            name: this.read_string()?,
                            properties: this.read_string()?,
                            position: this.read_vec3d()?,
                            radius: this.read_f32()?,
                        })
                    })?);
                    //println!("{:#?}", special_points);
                }
                b"EYE " => {
                    eye_points = Some(self.read_list(|this| {
                        Ok(EyePoint {
                            attached_subobj: ObjectId(this.read_u32()?),
                            offset: this.read_vec3d()?,
                            normal: this.read_vec3d()?,
                        })
                    })?);
                    //println!("{:#?}", eye_points);
                }
                b"GPNT" => {
                    primary_weps = Some(self.read_list(|this| {
                        Ok(this.read_list(|this| {
                            Ok(WeaponHardpoint {
                                position: this.read_vec3d()?,
                                normal: this.read_vec3d()?,
                                offset: (this.version >= Version::V22_01).then(|| this.read_f32().unwrap()).unwrap_or(0.0),
                            })
                        })?)
                    })?);
                    //println!("{:#?}", primary_weps);
                }
                b"MPNT" => {
                    secondary_weps = Some(self.read_list(|this| {
                        Ok(this.read_list(|this| {
                            Ok(WeaponHardpoint {
                                position: this.read_vec3d()?,
                                normal: this.read_vec3d()?,
                                offset: (this.version >= Version::V22_01).then(|| this.read_f32().unwrap()).unwrap_or(0.0),
                            })
                        })?)
                    })?);
                    //println!("{:#?}", secondary_weps);
                }
                b"TGUN" | b"TMIS" => {
                    turrets.extend(self.read_list(|this| {
                        Ok(Turret {
                            base_obj: ObjectId(this.read_u32()?),
                            gun_obj: ObjectId(this.read_u32()?),
                            normal: this.read_vec3d()?,
                            fire_points: this.read_list(|this| Ok(this.read_vec3d()?))?,
                        })
                    })?);
                    //println!("{:#?}", turrets);
                }
                b"FUEL" => {
                    assert!(thruster_banks.is_none());
                    thruster_banks = Some(self.read_list(|this| {
                        let num_glows = this.read_u32()?;
                        Ok(ThrusterBank {
                            properties: (this.version >= Version::V21_17).then(|| this.read_string().unwrap()).unwrap_or_default(),
                            glows: this.read_list_n(num_glows as usize, |this| {
                                Ok(ThrusterGlow {
                                    position: this.read_vec3d()?,
                                    normal: this.read_vec3d()?,
                                    radius: this.read_f32()?,
                                })
                            })?,
                        })
                    })?);
                    //println!("{:#?}", thruster_banks);
                }
                b"GLOW" => {
                    assert!(glow_banks.is_none());
                    glow_banks = Some(self.read_list(|this| {
                        let num_glow_points;
                        Ok(GlowPointBank {
                            disp_time: this.read_i32()?,
                            on_time: this.read_u32()?,
                            off_time: this.read_u32()?,
                            obj_parent: ObjectId(this.read_u32()?),
                            lod: this.read_u32()?,
                            glow_type: this.read_u32()?,
                            properties: {
                                num_glow_points = this.read_u32()?;
                                this.read_string()?
                            },
                            glow_points: this.read_list_n(num_glow_points as usize, |this| {
                                Ok(GlowPoint {
                                    position: this.read_vec3d()?,
                                    normal: this.read_vec3d()?,
                                    radius: this.read_f32()?,
                                })
                            })?,
                        })
                    })?);
                    //println!("{:#?}", glow_banks);
                }
                b"ACEN" => {
                    assert!(auto_center.is_none());
                    auto_center = Some(self.read_vec3d()?);
                }
                b"DOCK" => {
                    assert!(dock_points.is_none());
                    dock_points = Some(self.read_list(|this| {
                        Ok(Dock {
                            properties: this.read_string()?,
                            path: {
                                // spec allows for a list of paths but only the first will be used so dont bother
                                let paths = this.read_list(|this| Ok(this.read_u32()?))?;
                                paths.first().map(|&x| PathId(x))
                            },
                            points: {
                                // same thing here, only first 2 are used
                                let dockpoints =
                                    this.read_list(|this| Ok(DockingPoint { position: this.read_vec3d()?, normal: this.read_vec3d()? }))?;
                                assert!(dockpoints.len() < 3);
                                dockpoints
                            },
                        })
                    })?);
                    //println!("{:#?}", dock_points);
                }
                b"INSG" => {
                    assert!(insignias.is_none());
                    insignias = Some(self.read_list(|this| {
                        let num_faces;
                        Ok(Insignia {
                            detail_level: this.read_u32()?,
                            vertices: {
                                num_faces = this.read_u32()?;
                                this.read_list(|this| this.read_vec3d())?
                            },
                            offset: this.read_vec3d()?,
                            faces: this.read_list_n(num_faces as usize, |this| {
                                let [x, y, z] = *this.read_array(|this| {
                                    Ok(PolyVertex {
                                        vertex_id: VertexId(this.read_u32()?.try_into().unwrap()),
                                        normal_id: (),
                                        uv: (this.read_f32()?, this.read_f32()?),
                                    })
                                })?;
                                Ok((x, y, z))
                            })?,
                        })
                    })?);
                    //println!("{:#?}", insignias);
                }
                b"SHLD" => {
                    assert!(shield_data.is_none());
                    shield_data = Some((
                        self.read_list(|this| this.read_vec3d())?,
                        self.read_list(|this| {
                            Ok(ShieldPolygon {
                                normal: this.read_vec3d()?,
                                verts: (
                                    VertexId(this.read_u32()?.try_into().unwrap()),
                                    VertexId(this.read_u32()?.try_into().unwrap()),
                                    VertexId(this.read_u32()?.try_into().unwrap()),
                                ),
                                neighbors: (
                                    PolygonId(this.read_u32()?.try_into().unwrap()),
                                    PolygonId(this.read_u32()?.try_into().unwrap()),
                                    PolygonId(this.read_u32()?.try_into().unwrap()),
                                ),
                            })
                        })?,
                    ))
                }
                b"SLDC" => {
                    assert!(shield_tree_chunk.is_none());
                    // deal with this later, once we're sure to also have the shield data
                    shield_tree_chunk = Some(self.read_byte_buffer()?);
                }
                b"SLC2" => {
                    assert!(shield_tree_chunk.is_none());
                    assert!(self.version >= Version::V22_00);
                    // deal with this later, once we're sure to also have the shield data
                    shield_tree_chunk = Some(self.read_byte_buffer()?);
                }
                b"PINF" => {
                    assert!(comments.is_none());
                    // gotta inline some stuff because the length of this string is the length of the chunk
                    let mut buffer = vec![0; len as usize];
                    self.file.read_exact(&mut buffer)?;

                    let end = buffer.iter().position(|&char| char == 0).unwrap_or(buffer.len());
                    comments = Some(String::from_utf8(buffer[..end].into()).unwrap());
                    // println!("{:#?}", comments);
                }
                asd => {
                    eprintln!("I don't know how to handle id {:x?}", asd);
                    self.file.seek(SeekFrom::Current(len as i64))?;
                }
            }
        }

        // finally handle the shield tree, if applicable
        let shield_data = match (shield_data, shield_tree_chunk) {
            (Some((verts, poly_list)), shield_tree_chunk) => Some(ShieldData {
                verts,
                polygons: poly_list,
                collision_tree: shield_tree_chunk.map(|chunk| parse_shield_node(&chunk, self.version)),
            }),
            (None, Some(_)) => unreachable!(),
            _ => None,
        };
        //println!("{:#?}", shield_data);

        for i in 0..sub_objects.len() {
            if let Some(parent) = sub_objects.0[i].parent {
                let id = sub_objects.0[i].obj_id;
                sub_objects[parent].children.push(id);
            }
        }

        for id in debris_objs {
            sub_objects[id].is_debris_model = true;
        }

        Ok(Model {
            header: header.expect("No header chunk found???"),
            sub_objects,
            textures: textures.unwrap_or_default(),
            paths: paths.unwrap_or_default(),
            special_points: special_points.unwrap_or_default(),
            eye_points: eye_points.unwrap_or_default(),
            primary_weps: primary_weps.unwrap_or_default(),
            secondary_weps: secondary_weps.unwrap_or_default(),
            turrets,
            thruster_banks: thruster_banks.unwrap_or_default(),
            comments: comments.unwrap_or_default(),
            docking_bays: dock_points.unwrap_or_default(),
            insignias: insignias.unwrap_or_default(),
            glow_banks: glow_banks.unwrap_or_default(),
            auto_center: auto_center.unwrap_or_default(),
            shield_data,
        })
    }

    fn read_list<T>(&mut self, f: impl FnMut(&mut Self) -> io::Result<T>) -> io::Result<Vec<T>> {
        let n = self.read_u32()? as usize;
        self.read_list_n(n, f)
    }

    fn read_list_n<T>(&mut self, n: usize, mut f: impl FnMut(&mut Self) -> io::Result<T>) -> io::Result<Vec<T>> {
        (0..n).map(|_| f(self)).collect()
    }

    fn read_array<T, const N: usize>(&mut self, f: impl FnMut(&mut Self) -> io::Result<T>) -> io::Result<Box<[T; N]>> {
        Ok(self.read_list_n(N, f)?.into_boxed_slice().try_into().ok().unwrap())
    }

    fn read_string(&mut self) -> io::Result<String> {
        let buf = self.read_byte_buffer()?;
        let end = buf.iter().position(|&char| char == 0).unwrap_or(buf.len());
        Ok(String::from_utf8(buf[..end].into()).unwrap())
    }

    fn read_u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.read_bytes()?))
    }

    fn read_i32(&mut self) -> io::Result<i32> {
        read_i32(&mut self.file)
    }

    fn read_byte_buffer(&mut self) -> io::Result<Box<[u8]>> {
        let mut buffer = vec![0; self.read_u32()? as usize];
        //println!("buffer size is {}", buffer.len());
        self.file.read_exact(&mut buffer)?;

        Ok(buffer.into())
    }

    fn read_bytes<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        read_bytes(&mut self.file)
    }

    fn read_bbox(&mut self) -> io::Result<BBox> {
        Ok(BBox { min: self.read_vec3d()?, max: self.read_vec3d()? })
    }

    fn read_vec3d(&mut self) -> io::Result<Vec3d> {
        Ok(Vec3d {
            x: self.read_f32()?,
            y: self.read_f32()?,
            z: self.read_f32()?,
        })
    }

    fn read_f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.read_bytes()?))
    }
}

fn read_i32(file: &mut impl Read) -> io::Result<i32> {
    Ok(i32::from_le_bytes(read_bytes(file)?))
}

fn read_bytes<const N: usize>(file: &mut impl Read) -> io::Result<[u8; N]> {
    let mut buffer = [0; N];
    file.read_exact(&mut buffer)?;

    Ok(buffer)
}

fn read_list_n<T>(n: usize, buf: &mut &[u8], mut f: impl FnMut(&mut &[u8]) -> T) -> Vec<T> {
    (0..n).map(|_| f(buf)).collect()
}

fn read_vec3d(buf: &mut &[u8]) -> Vec3d {
    Vec3d {
        x: buf.read_f32::<LE>().unwrap(),
        y: buf.read_f32::<LE>().unwrap(),
        z: buf.read_f32::<LE>().unwrap(),
    }
}

fn parse_chunk_header(buf: &[u8], chunk_type_is_u8: bool) -> (u32, &[u8], &[u8]) {
    let mut pointer = buf;
    let chunk_type = if chunk_type_is_u8 {
        pointer.read_u8().unwrap().into()
    } else {
        pointer.read_u32::<LE>().unwrap()
    };

    /*println!("found a {}", match chunk_type {
        ENDOFBRANCH => "ENDOFBRANCH",
        DEFFPOINTS => "DEFFPOINTS",
        FLATPOLY => "FLATPOLY",
        TMAPPOLY => "TMAPPOLY",
        SORTNORM => "SORTNORM",
        BOUNDBOX => "BOUNDBOX",
        _ => "no i dont"
    });*/
    /*println!(
        "found a {}",
        match chunk_type {
            0 => "split",
            1 => "leaf",
            _ => "dunno lol",
        }
    );*/
    let chunk_size = pointer.read_u32::<LE>().unwrap() as usize;
    (chunk_type, pointer, &buf[chunk_size..])
}

fn parse_bsp_data(mut buf: &[u8]) -> BspData {
    fn parse_bsp_node(mut buf: &[u8]) -> BspNode {
        let read_bbox = |chunk: &mut &[u8]| BBox { min: read_vec3d(chunk), max: read_vec3d(chunk) };

        // parse the first header
        let (chunk_type, mut chunk, next_chunk) = parse_chunk_header(buf, false);
        // the first chunk (after deffpoints) AND the chunks pointed to be SORTNORM's front and back branches should ALWAYS be either another
        // SORTNORM or a BOUNDBOX followed by some polygons
        //dbg!(chunk_type);
        match chunk_type {
            BspData::SORTNORM => BspNode::Split {
                normal: read_vec3d(&mut chunk),
                point: read_vec3d(&mut chunk),
                front: {
                    let _reserved = chunk.read_u32::<LE>().unwrap(); // just to advance past it
                    let offset = chunk.read_u32::<LE>().unwrap();
                    assert!(offset != 0);
                    Box::new(parse_bsp_node(&buf[offset as usize..]))
                },
                back: {
                    let offset = chunk.read_u32::<LE>().unwrap();
                    assert!(offset != 0);
                    Box::new(parse_bsp_node(&buf[offset as usize..]))
                },
                bbox: {
                    let _prelist = chunk.read_u32::<LE>().unwrap(); //
                    let _postlist = chunk.read_u32::<LE>().unwrap(); // All 3 completely unused, as far as i can tell
                    let _online = chunk.read_u32::<LE>().unwrap(); //
                    assert!(_prelist == 0 || buf[_prelist as usize] == 0); //
                    assert!(_postlist == 0 || buf[_postlist as usize] == 0); // And so let's make sure thats the case, they should all lead to ENDOFBRANCH
                    assert!(_online == 0 || buf[_online as usize] == 0); //
                    read_bbox(&mut chunk)
                },
            },
            BspData::BOUNDBOX => BspNode::Leaf {
                bbox: read_bbox(&mut chunk),
                poly_list: {
                    let mut poly_list = vec![];
                    buf = next_chunk;
                    loop {
                        let (chunk_type, mut chunk, next_chunk) = parse_chunk_header(buf, false);
                        // keeping looping and pushing new polygons
                        poly_list.push(match chunk_type {
                            BspData::TMAPPOLY => {
                                let normal = read_vec3d(&mut chunk);
                                let center = read_vec3d(&mut chunk);
                                let radius = chunk.read_f32::<LE>().unwrap();
                                let num_verts = chunk.read_u32::<LE>().unwrap();
                                let texture = Texturing::Texture(TextureId(chunk.read_u32::<LE>().unwrap()));
                                let verts = read_list_n(num_verts as usize, &mut chunk, |chunk| PolyVertex {
                                    vertex_id: VertexId(chunk.read_u16::<LE>().unwrap()),
                                    normal_id: NormalId(chunk.read_u16::<LE>().unwrap()),
                                    uv: (chunk.read_f32::<LE>().unwrap(), chunk.read_f32::<LE>().unwrap()),
                                });

                                Polygon { normal, center, radius, verts, texture }
                            }
                            BspData::FLATPOLY => {
                                let normal = read_vec3d(&mut chunk);
                                let center = read_vec3d(&mut chunk);
                                let radius = chunk.read_f32::<LE>().unwrap();
                                let num_verts = chunk.read_u32::<LE>().unwrap();
                                let texture = Texturing::Flat(Color {
                                    red: chunk.read_u8().unwrap(),
                                    green: chunk.read_u8().unwrap(),
                                    blue: chunk.read_u8().unwrap(),
                                });
                                let _ = chunk.read_u8().unwrap(); // get rid of padding byte
                                let verts = read_list_n(num_verts as usize, &mut chunk, |chunk| PolyVertex {
                                    vertex_id: VertexId(chunk.read_u16::<LE>().unwrap()),
                                    normal_id: NormalId(chunk.read_u16::<LE>().unwrap()),
                                    uv: Default::default(),
                                });

                                Polygon { normal, center, radius, verts, texture }
                            }
                            BspData::ENDOFBRANCH => {
                                break;
                            }
                            _ => {
                                unreachable!("unknown chunk type! {}", chunk_type);
                            }
                        });

                        buf = next_chunk;
                    }
                    //assert!(!poly_list.is_empty());
                    //println!("leaf length {}", poly_list.len());
                    poly_list
                },
            },
            _ => {
                unreachable!();
            }
        }
    }

    //println!("started parsing a bsp tree");

    let (chunk_type, mut chunk, next_chunk) = parse_chunk_header(buf, false);
    assert!(chunk_type == BspData::DEFFPOINTS);

    let num_verts = chunk.read_u32::<LE>().unwrap();
    let num_norms = chunk.read_u32::<LE>().unwrap();
    let offset = chunk.read_u32::<LE>().unwrap();
    let norm_counts = &chunk[0..num_verts as usize];

    buf = &buf[offset as usize..];

    let mut verts = vec![];
    let mut norms = vec![];
    for &count in norm_counts {
        verts.push(read_vec3d(&mut buf));
        for _ in 0..count {
            norms.push(read_vec3d(&mut buf));
        }
    }

    assert!(num_norms as usize == norms.len());

    let bsp_tree = parse_bsp_node(next_chunk);

    BspData { collision_tree: bsp_tree, norms, verts }
}

fn parse_shield_node(buf: &[u8], version: Version) -> ShieldNode {
    let read_bbox = |chunk: &mut &[u8]| BBox { min: read_vec3d(chunk), max: read_vec3d(chunk) };

    let (chunk_type, mut chunk, _) = parse_chunk_header(buf, version <= Version::V21_17);
    match chunk_type {
        ShieldNode::SPLIT => ShieldNode::Split {
            bbox: read_bbox(&mut chunk),
            front: {
                let offset = chunk.read_u32::<LE>().unwrap();
                assert!(offset != 0);
                Box::new(parse_shield_node(&buf[offset as usize..], version))
            },
            back: {
                let offset = chunk.read_u32::<LE>().unwrap();
                assert!(offset != 0);
                Box::new(parse_shield_node(&buf[offset as usize..], version))
            },
        },
        ShieldNode::LEAF => ShieldNode::Leaf {
            bbox: Some(read_bbox(&mut chunk)),
            poly_list: read_list_n(chunk.read_u32::<LE>().unwrap() as usize, &mut chunk, |chunk| PolygonId(chunk.read_u32::<LE>().unwrap())),
        },
        _ => unreachable!(),
    }
}
